use crate::builtin::surface;
use crate::hir::{
    BlockId, ConstId, ExprId, ExprKind, FunctionId, Module, ParamId, PatternKind, PlaceId,
    PlaceKind, StmtId, StmtKind,
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
    closures: Vec<(
        ExprId,
        std::collections::HashSet<ResolvedName>,
        Vec<ResolvedName>,
    )>,
}

impl<'a> BodyResolver<'a> {
    pub(crate) fn new(
        names: &'a std::sync::Arc<NameTable>,
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
            closures: Vec::new(),
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
                self.resolve_expr(condition.value());
                if let crate::hir::Condition::Binding { pattern, .. } = condition {
                    self.push_child_scope(self.source_map.block_span(*body));
                    self.bind_pattern(*pattern, self.source_map.block_span(*body).start);
                }
                self.resolve_block(*body);
                if matches!(condition, crate::hir::Condition::Binding { .. }) {
                    self.pop_scope();
                }
            }
            StmtKind::Loop { body } => self.resolve_block(*body),
            StmtKind::For {
                pattern,
                iterable,
                body,
            } => {
                self.resolve_expr(*iterable);
                self.push_child_scope(self.source_map.stmt_span(stmt_id));
                self.bind_pattern(*pattern, self.source_map.stmt_span(stmt_id).start);
                self.resolve_block(*body);
                self.pop_scope();
            }
            StmtKind::Expr(expr) => self.resolve_expr(*expr),
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::BreakValue(expr) => self.resolve_expr(*expr),
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
            ExprKind::Name { name, .. } => {
                if let Some(resolved) = self.resolve_name(name) {
                    self.resolved.insert_expr(expr_id, resolved);
                    self.record_capture(resolved);
                } else if let Some((owner, member)) = name.rsplit_once("::")
                    && let Some(owner) = self.resolve_name(owner)
                {
                    self.resolved.insert_qualified_member(
                        expr_id,
                        crate::resolver::QualifiedMember {
                            owner,
                            name: member.to_owned(),
                        },
                    );
                }
            }
            ExprKind::Literal(_) => {}
            ExprKind::Prefix { expr, .. } => self.resolve_expr(*expr),
            ExprKind::Binary { lhs, rhs, .. }
            | ExprKind::Range {
                start: lhs,
                end: rhs,
                ..
            } => {
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
                self.resolve_expr(condition.value());
                if let crate::hir::Condition::Binding { pattern, .. } = condition {
                    self.push_child_scope(self.source_map.block_span(*then_branch));
                    self.bind_pattern(*pattern, self.source_map.block_span(*then_branch).start);
                }
                self.resolve_block(*then_branch);
                if matches!(condition, crate::hir::Condition::Binding { .. }) {
                    self.pop_scope();
                }
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
                    self.bind_pattern(arm.pattern, span.start);
                    if let Some(guard) = arm.guard {
                        self.resolve_expr(guard);
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
            ExprKind::Loop { body } => self.resolve_block(*body),
            ExprKind::Closure { params, body } => {
                let outer = self
                    .scopes
                    .iter()
                    .flat_map(|scope| {
                        self.resolved.scopes[scope.id]
                            .bindings
                            .iter()
                            .map(|binding| binding.resolved)
                    })
                    .collect();
                self.closures.push((expr_id, outer, Vec::new()));
                self.push_child_scope(self.source_map.expr_span(expr_id));
                for param in params {
                    self.bind_name(
                        &param.name,
                        ResolvedName::Local(param.local),
                        self.source_map.expr_span(*body).start,
                    );
                }
                self.resolve_expr(*body);
                self.pop_scope();
                let (_, _, captures) = self.closures.pop().expect("closure context");
                self.resolved.insert_closure_captures(expr_id, captures);
            }
        }
    }

    fn bind_pattern(&mut self, pattern: crate::hir::PatternId, start: usize) {
        match &self.module.pattern(pattern).kind {
            PatternKind::Or(alternatives) => {
                if let Some(first) = alternatives.first() {
                    self.bind_pattern(*first, start);
                }
            }
            PatternKind::Name { name, local } if !name.is_empty() && name != "<missing>" => {
                let name = name.clone();
                let local = *local;
                self.bind_name(&name, ResolvedName::Local(local), start);
            }
            PatternKind::Tuple(elements) => {
                let elements = elements.clone();
                for element in elements {
                    self.bind_pattern(element, start);
                }
            }
            PatternKind::Struct { fields, .. } => {
                let fields = fields.clone();
                for field in fields {
                    self.bind_pattern(field.pattern, start);
                }
            }
            PatternKind::EnumVariant { fields, .. } => {
                let fields = fields.clone();
                for field in fields {
                    self.bind_pattern(field, start);
                }
            }
            _ => {}
        }
    }

    fn resolve_place(&mut self, place_id: PlaceId) {
        self.assert_current_owner(place_id.owner());
        let place = self.module.place(place_id);
        match &place.kind {
            PlaceKind::Name(name) => {
                if let Some(resolved) = self.resolve_name(name) {
                    self.resolved.insert_place(place_id, resolved);
                    self.record_capture(resolved);
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

    fn record_capture(&mut self, resolved: ResolvedName) {
        if !matches!(resolved, ResolvedName::Local(_) | ResolvedName::Param(_)) {
            return;
        }
        for (_, outer, captures) in &mut self.closures {
            if outer.contains(&resolved) && !captures.contains(&resolved) {
                captures.push(resolved);
            }
        }
    }

    fn binding(&self, name: &str) -> Option<super::NameResolution> {
        for scope in self.scopes.iter().rev() {
            if let Some(index) = scope.latest.get(name) {
                return Some(super::NameResolution::Unique(
                    self.resolved.scopes[scope.id].bindings[*index].resolved,
                ));
            }
        }
        self.names.lookup(name)
    }

    fn resolve_name(&self, name: &str) -> Option<ResolvedName> {
        if let Some(binding) = self.binding(name) {
            return binding.target();
        }
        if let Some((alias, member)) = name.split_once("::")
            && let Some(binding) = self.binding(alias)
        {
            return match binding.target()? {
                ResolvedName::SourceImport(index) => {
                    self.resolved
                        .imports
                        .resolve_member(index, member, &self.resolved.hosts)
                }
                ResolvedName::Module(id) => {
                    let index = *self.resolved.imports.module_aliases.get(&id)?;
                    self.resolved
                        .imports
                        .resolve_member(index, member, &self.resolved.hosts)
                }
                ResolvedName::HostModule(module) => {
                    self.resolved.hosts.resolve_name_in(module, member)
                }
                ResolvedName::StandardModule(module) => surface::standard_function(module, member)
                    .map(|f| ResolvedName::StandardFunction(f.intrinsic)),
                _ => None,
            };
        }
        if let Some(resolved) = self.resolved.hosts.resolve_name(name) {
            return Some(resolved);
        }
        if let Some(module) = surface::standard_module(name) {
            return Some(ResolvedName::StandardModule(module.kind));
        }
        if let Some(helper) = crate::builtin::BuiltinFunction::from_name(name) {
            return Some(ResolvedName::RuntimeHelper(helper));
        }
        let (module, member) = name.rsplit_once("::")?;
        surface::standard_function(surface::standard_module(module)?.kind, member)
            .map(|f| ResolvedName::StandardFunction(f.intrinsic))
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
