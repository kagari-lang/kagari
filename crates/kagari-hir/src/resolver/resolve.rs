use crate::builtin::surface;
use crate::hir::{
    BlockId, ConstId, EnumId, ExprId, ExprKind, FunctionId, Module, ModuleId, ParamId, PatternKind,
    PlaceId, PlaceKind, StmtId, StmtKind, StructId, TraitId,
};
use crate::hir::{BodyOwner, HirOwner};
use crate::resolver::{LexicalScope, ScopeBinding};
use crate::resolver::{ResolvedName, ResolvedNames, table::NameTable};
use crate::source_map::SourceMap;
use kagari_common::Span;
use std::collections::HashMap;

struct ActiveScope {
    id: usize,
    latest: HashMap<String, usize>,
}

pub(crate) struct BodyResolver<'a> {
    cancel: kagari_common::cancellation::CancellationToken,
    names: &'a NameTable,
    module: &'a Module,
    resolved: ResolvedNames,
    source_map: &'a SourceMap,
    scopes: Vec<ActiveScope>,
}

impl<'a> BodyResolver<'a> {
    pub(crate) fn new(
        names: &'a NameTable,
        module: &'a Module,
        source_map: &'a SourceMap,
        hosts: std::sync::Arc<crate::host::HostDeclarations>,
        imports: std::sync::Arc<crate::imports::ModuleImports>,
        cancel: kagari_common::cancellation::CancellationToken,
    ) -> Self {
        Self {
            cancel,
            names,
            module,
            source_map,
            resolved: ResolvedNames::new(names.clone(), hosts, imports),
            scopes: Vec::new(),
        }
    }

    pub(crate) fn finish(self) -> ResolvedNames {
        self.resolved
    }

    pub(crate) fn resolve_function(
        &mut self,
        function: FunctionId,
        params: impl Iterator<Item = (&'a str, ParamId)>,
        body: BlockId,
    ) {
        assert_eq!(
            body.owner(),
            HirOwner::Body(BodyOwner::Function(function)),
            "function body owner mismatch"
        );
        let span = self.source_map.block_span(body);
        self.push_scope(
            self.source_map.function_span(function),
            BodyOwner::Function(function),
        );
        if self.module.module_init == Some(function) {
            let scope = self.scopes.last().expect("initializer scope").id;
            self.resolved.scopes[scope].excluded_ranges = self
                .module
                .items
                .iter()
                .take_while(|_| self.cancel.check().is_ok())
                .map(|item| self.source_map.item_span(*item))
                .collect();
        }
        for (name, id) in params {
            self.bind_name(name, ResolvedName::Param(id), span.start);
        }
        self.resolve_block(body);
        self.pop_scope();
    }

    pub(crate) fn resolve_top_level_expr(&mut self, owner: ConstId, expr: ExprId) {
        assert_eq!(
            expr.owner(),
            HirOwner::Body(BodyOwner::Const(owner)),
            "constant body owner mismatch"
        );
        self.push_scope(self.source_map.expr_span(expr), BodyOwner::Const(owner));
        self.resolve_expr(expr);
        self.pop_scope();
    }

    fn resolve_block(&mut self, block_id: BlockId) {
        self.assert_current_owner(block_id.owner());
        let block = self.module.block(block_id);
        self.push_child_scope(self.source_map.block_span(block_id));
        for stmt in &block.statements {
            if self.cancel.check().is_err() {
                break;
            }
            self.resolve_stmt(*stmt);
        }
        if let Some(expr) = block.tail_expr {
            self.resolve_expr(expr);
        }
        self.pop_scope();
    }

    fn resolve_stmt(&mut self, stmt_id: StmtId) {
        self.assert_current_owner(stmt_id.owner());
        if self.cancel.check().is_err() {
            return;
        }
        let stmt = self.module.stmt(stmt_id);
        match &stmt.kind {
            StmtKind::Binding {
                local,
                name,
                initializer,
                ..
            } => {
                self.resolve_expr(*initializer);
                if !name.is_empty() {
                    self.bind_name(
                        name,
                        ResolvedName::Local(*local),
                        self.source_map.stmt_span(stmt_id).end,
                    );
                }
            }
            StmtKind::Assign { target, value, .. } => {
                self.resolve_place(*target);
                self.resolve_expr(*value);
            }
            StmtKind::Return { expr } => {
                if let Some(expr) = expr {
                    self.resolve_expr(*expr);
                }
            }
            StmtKind::While { condition, body } => {
                self.resolve_expr(*condition);
                self.resolve_block(*body);
            }
            StmtKind::Loop { body } => self.resolve_block(*body),
            StmtKind::Expr(expr) => self.resolve_expr(*expr),
            StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn resolve_expr(&mut self, expr_id: ExprId) {
        self.assert_current_owner(expr_id.owner());
        if self.cancel.check().is_err() {
            return;
        }
        let expr = self.module.expr(expr_id);
        match &expr.kind {
            ExprKind::Missing => {}
            ExprKind::Name(name) => {
                if let Some(resolved) = self.resolve_name(name) {
                    self.resolved.insert_expr(expr_id, resolved);
                }
            }
            ExprKind::Literal(_) => {}
            ExprKind::Prefix { expr, .. } => self.resolve_expr(*expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.resolve_expr(*lhs);
                self.resolve_expr(*rhs);
            }
            ExprKind::Call { callee, args } => {
                self.resolve_expr(*callee);
                for arg in args {
                    self.resolve_expr(*arg);
                }
            }
            ExprKind::Field { receiver, .. } => self.resolve_expr(*receiver),
            ExprKind::Index { receiver, index } => {
                self.resolve_expr(*receiver);
                self.resolve_expr(*index);
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.resolve_expr(*condition);
                self.resolve_block(*then_branch);
                if let Some(expr) = else_branch {
                    self.resolve_expr(*expr);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.resolve_expr(*scrutinee);
                for arm in arms {
                    self.assert_current_owner(arm.pattern.owner());
                    let span = self.source_map.expr_span(arm.expr);
                    self.push_child_scope(span);
                    if let PatternKind::Name { name, local } =
                        &self.module.pattern(arm.pattern).kind
                        && !name.is_empty()
                        && name != "<missing>"
                    {
                        self.bind_name(name, ResolvedName::Local(*local), span.start);
                    }
                    self.resolve_expr(arm.expr);
                    self.pop_scope();
                }
            }
            ExprKind::StructInit { fields, .. } => {
                for field in fields {
                    self.resolve_expr(field.value);
                }
            }
            ExprKind::Tuple(elements) | ExprKind::Array(elements) => {
                for expr in elements {
                    self.resolve_expr(*expr);
                }
            }
            ExprKind::Block(block) => self.resolve_block(*block),
        }
    }

    fn resolve_place(&mut self, place_id: PlaceId) {
        self.assert_current_owner(place_id.owner());
        let place = self.module.place(place_id);
        match &place.kind {
            PlaceKind::Name(name) => {
                if let Some(resolved) = self.resolve_name(name) {
                    self.resolved.insert_place(place_id, resolved);
                }
            }
            PlaceKind::Expr(expr) => self.resolve_expr(*expr),
            PlaceKind::Field { base, .. } => self.resolve_place(*base),
            PlaceKind::Index { base, index } => {
                self.resolve_place(*base);
                self.resolve_expr(*index);
            }
        }
    }

    fn resolve_name(&self, name: &str) -> Option<ResolvedName> {
        for scope in self.scopes.iter().rev() {
            if let Some(index) = scope.latest.get(name) {
                return Some(self.resolved.scopes[scope.id].bindings[*index].resolved);
            }
        }

        if let Some(id) = self.names.function(name) {
            return Some(ResolvedName::Function(id));
        }
        if let Some(id) = self.names.host_functions.get(name) {
            return Some(ResolvedName::HostFunction(*id));
        }
        if let Some(index) = self.names.source_imports.get(name) {
            return Some(ResolvedName::SourceImport(*index));
        }
        if let Some((alias, member)) = name.split_once("::")
            && let Some(index) = self.names.source_imports.get(alias)
            && let Some(crate::imports::ImportTarget::Source(target)) =
                &self.resolved.imports.entries[*index].target
            && target.item.is_none()
            && let Some(items) = target.members.get(member)
            && let [item] = items.as_slice()
        {
            return Some(ResolvedName::SourceItem {
                import: *index,
                item: *item,
            });
        }
        if let Some(id) = self.names.host_modules.get(name) {
            return Some(ResolvedName::HostModule(*id));
        }
        if let Some((alias, suffix)) = name.split_once("::")
            && let Some(module) = self.names.host_modules.get(alias)
            && let Some(id) = self.resolved.hosts.resolve_in(*module, suffix)
        {
            return Some(ResolvedName::HostFunction(id));
        }
        if let Some(id) = self.resolved.hosts.resolve(name) {
            return Some(ResolvedName::HostFunction(id));
        }
        if let Some(id) = self.names.const_(name) {
            return Some(ResolvedName::Const(id));
        }
        if let Some(id) = self.names.module(name) {
            return Some(ResolvedName::Module(id));
        }
        if let Some(module) = self.names.standard_module(name) {
            return Some(ResolvedName::StandardModule(module));
        }
        if let Some(intrinsic) = self.names.standard_function(name) {
            return Some(ResolvedName::StandardFunction(intrinsic));
        }
        if let Some(id) = self.names.struct_(name) {
            return Some(ResolvedName::Struct(id));
        }
        if let Some(id) = self.names.enum_(name) {
            return Some(ResolvedName::Enum(id));
        }
        self.names.trait_(name).map(ResolvedName::Trait)
    }

    fn bind_name(&mut self, name: &str, resolved: ResolvedName, visible_from: usize) {
        match resolved {
            ResolvedName::Param(id) => self.assert_current_owner(id.owner()),
            ResolvedName::Local(id) => self.assert_current_owner(id.owner()),
            _ => unreachable!("only local bindings enter a body scope"),
        }
        if let Some(scope) = self.scopes.last_mut() {
            let bindings = &mut self.resolved.scopes[scope.id].bindings;
            scope.latest.insert(name.to_owned(), bindings.len());
            bindings.push(ScopeBinding {
                name: name.to_owned(),
                resolved,
                visible_from,
            });
        }
    }

    fn push_scope(&mut self, span: Span, owner: BodyOwner) {
        let id = self.resolved.scopes.len();
        self.resolved.scopes.push(LexicalScope {
            owner,
            span,
            parent: self.scopes.last().map(|scope| scope.id),
            bindings: Vec::new(),
            excluded_ranges: Vec::new(),
        });
        self.scopes.push(ActiveScope {
            id,
            latest: HashMap::new(),
        });
    }

    fn push_child_scope(&mut self, span: Span) {
        let owner =
            self.resolved.scopes[self.scopes.last().expect("body owns its scopes").id].owner;
        self.push_scope(span, owner);
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn assert_current_owner(&self, owner: HirOwner) {
        let current =
            self.resolved.scopes[self.scopes.last().expect("body owns its nodes").id].owner;
        assert_eq!(
            owner,
            HirOwner::Body(current),
            "HIR traversal crossed a body boundary"
        );
    }
}

trait TopLevelLookup {
    fn function(&self, name: &str) -> Option<FunctionId>;
    fn const_(&self, name: &str) -> Option<ConstId>;
    fn module(&self, name: &str) -> Option<ModuleId>;
    fn standard_module(&self, name: &str) -> Option<surface::StandardModule>;
    fn standard_function(&self, name: &str) -> Option<surface::StandardIntrinsic>;
    fn struct_(&self, name: &str) -> Option<StructId>;
    fn enum_(&self, name: &str) -> Option<EnumId>;
    fn trait_(&self, name: &str) -> Option<TraitId>;
}

impl TopLevelLookup for NameTable {
    fn function(&self, name: &str) -> Option<FunctionId> {
        self.functions.get(name).copied()
    }

    fn const_(&self, name: &str) -> Option<ConstId> {
        self.consts.get(name).copied()
    }

    fn module(&self, name: &str) -> Option<ModuleId> {
        self.modules.get(name).copied()
    }

    fn standard_module(&self, name: &str) -> Option<surface::StandardModule> {
        self.standard_modules.get(name).copied()
    }

    fn standard_function(&self, name: &str) -> Option<surface::StandardIntrinsic> {
        self.standard_functions.get(name).copied()
    }

    fn struct_(&self, name: &str) -> Option<StructId> {
        self.structs.get(name).copied()
    }

    fn enum_(&self, name: &str) -> Option<EnumId> {
        self.enums.get(name).copied()
    }

    fn trait_(&self, name: &str) -> Option<TraitId> {
        self.traits.get(name).copied()
    }
}
