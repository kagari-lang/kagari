//! Lexical traversal, shadowing, namespace lookup and closure capture collection.

use crate::{
    builtin::BuiltinFunction,
    hir::{
        expr::{ClosureParam, Condition, ExprKind},
        ids::{
            BlockId, BodyOwner, ExprId, FunctionId, HirOwner, ParamId, PatternId, PlaceId, StmtId,
        },
        item::Module,
        pattern::PatternKind,
        place::PlaceKind,
        stmt::StmtKind,
        writeability::Writeability,
    },
    imports::{ModuleImportFacts, SourceItem, catalog::NamespaceCatalog},
    resolver::{
        resolved::{Binding, QualifiedMember, ResolvedName, ResolvedNames},
        table::{NameResolution, NameTable},
    },
};
use kagari_types::declaration::names::NameNamespace;

use kagari_common::cancellation::CancellationToken;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Disposable semantic frame; snapshots freeze named-local-function environments.
#[derive(Clone)]
struct ActiveScope {
    owner: BodyOwner,
    latest: HashMap<String, ResolvedName>,
}

/// Mutable lexical traversal state for one lowering and shared module inputs.
///
/// `scopes` implements current shadowing and is discarded after resolution.
/// Closure frames accumulate captured bindings, and owner assertions prevent
/// cross-function node traversal. No source-coordinate scope history is published.
pub(crate) struct BodyResolver<'a> {
    cancel: CancellationToken,
    names: &'a NameTable,
    module: &'a Module,
    resolved: ResolvedNames,
    scopes: Vec<ActiveScope>,
    closures: Vec<(ExprId, HashSet<ResolvedName>, Vec<ResolvedName>)>,
    local_scopes: HashMap<FunctionId, Vec<ActiveScope>>,
}

impl<'a> BodyResolver<'a> {
    pub(crate) fn new(
        names: &'a Arc<NameTable>,
        module: &'a Module,
        imports: Arc<ModuleImportFacts>,
        catalog: Arc<NamespaceCatalog>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            cancel,
            names,
            module,
            resolved: ResolvedNames::new(names.clone(), imports, catalog),
            scopes: Vec::new(),
            closures: Vec::new(),
            local_scopes: HashMap::new(),
        }
    }

    pub(crate) fn finish(self) -> ResolvedNames {
        self.resolved
    }

    /// Checks the body owner, binds parameters, visits its nested block scopes and restores the outer scope.
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
        self.scopes = self.local_scopes.remove(&function).unwrap_or_default();
        self.push_scope(BodyOwner::Function(function));
        for (name, id) in params {
            self.bind_name(name, ResolvedName::Param(id), Writeability::Val);
        }
        self.resolve_block(body);
        self.pop_scope();
        self.scopes.clear();
    }

    /// Resolves an initializer under its asserted constant or runtime static owner.
    pub(crate) fn resolve_top_level_expr(&mut self, owner: BodyOwner, expr: ExprId) {
        assert_eq!(
            expr.owner(),
            HirOwner::Body(owner),
            "initializer body owner mismatch"
        );
        self.push_scope(owner);
        self.resolve_expr(expr);
        self.pop_scope();
    }

    fn resolve_block(&mut self, block_id: BlockId) {
        self.assert_current_owner(block_id.owner());
        let block = self.module.block(block_id);
        self.push_child_scope();
        for stmt in &block.statements {
            if self.cancel.check().is_err() {
                break;
            }
            if let StmtKind::Function(id) = self.module.stmt(*stmt).kind {
                let function = &self.module.functions[id.index()];
                self.bind_name(
                    &function.name,
                    ResolvedName::Function(id),
                    Writeability::Val,
                );
            }
        }
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
                pattern,
                initializer,
                writeability,
                ..
            } => {
                self.resolve_expr(*initializer);
                self.resolve_pattern_variants(*pattern, false);

                for (local, name) in self.module.nodes.pattern_bindings(*pattern) {
                    if !name.is_empty() && name != "<missing>" {
                        self.bind_name(name, ResolvedName::Local(local), *writeability);
                    }
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
                if let Condition::Binding { pattern, .. } = condition {
                    self.push_child_scope();
                    self.bind_pattern(*pattern);
                }
                self.resolve_block(*body);
                if matches!(condition, Condition::Binding { .. }) {
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
                self.push_child_scope();
                self.bind_pattern(*pattern);
                self.resolve_block(*body);
                self.pop_scope();
            }
            StmtKind::Expr(expr) => self.resolve_expr(*expr),
            StmtKind::Function(id) => {
                self.local_scopes.insert(*id, self.scopes.clone());
            }
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
                if let Some(resolved) = self.resolve_name(name, NameNamespace::Value) {
                    self.resolved.insert_expr(expr_id, resolved.clone());

                    self.record_capture(resolved);
                } else if let Some((owner, member)) = name.rsplit_once("::")
                    && let Some(owner) = self.resolve_name(owner, NameNamespace::Type)
                {
                    self.resolved.insert_qualified_member(
                        expr_id,
                        QualifiedMember {
                            owner,
                            name: member.to_owned(),
                        },
                    );
                }
            }
            ExprKind::Literal(_) => {}
            ExprKind::FormatPart { expr, .. }
            | ExprKind::Await { expr }
            | ExprKind::Propagate { expr }
            | ExprKind::Cast { expr, .. }
            | ExprKind::Prefix { expr, .. } => self.resolve_expr(*expr),
            ExprKind::Binary { lhs, rhs, .. }
            | ExprKind::ArrayRepeat {
                value: lhs,
                count: rhs,
            } => {
                self.resolve_expr(*lhs);
                self.resolve_expr(*rhs);
            }
            ExprKind::Range { start, end, .. } => {
                for expr in start.iter().chain(end) {
                    self.resolve_expr(*expr);
                }
            }
            ExprKind::Call { callee, args, .. } => {
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
                if let Condition::Binding { pattern, .. } = condition {
                    self.push_child_scope();
                    self.bind_pattern(*pattern);
                }
                self.resolve_block(*then_branch);
                if matches!(condition, Condition::Binding { .. }) {
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
                    self.push_child_scope();
                    self.bind_pattern(arm.pattern);
                    if let Some(guard) = arm.guard {
                        self.resolve_expr(guard);
                    }
                    self.resolve_expr(arm.expr);
                    self.pop_scope();
                }
            }
            ExprKind::StructInit { fields, base, .. } => {
                for field in fields {
                    self.resolve_expr(field.value);
                }
                if let Some(base) = base {
                    self.resolve_expr(*base);
                }
            }
            ExprKind::InterpolatedString(elements)
            | ExprKind::Tuple(elements)
            | ExprKind::Array { elements, .. } => {
                for expr in elements {
                    self.resolve_expr(*expr);
                }
            }
            ExprKind::Block(block) => self.resolve_block(*block),
            ExprKind::Loop { body } => self.resolve_block(*body),
            ExprKind::Closure { params, body, .. } => {
                self.resolve_closure(expr_id, params, *body);
            }
            ExprKind::AsyncBlock { body } => self.resolve_closure(expr_id, &[], *body),
        }
    }

    fn resolve_closure(&mut self, expr_id: ExprId, params: &[ClosureParam], body: ExprId) {
        let outer = self
            .scopes
            .iter()
            .flat_map(|scope| scope.latest.values().cloned())
            .collect();
        self.closures.push((expr_id, outer, Vec::new()));
        self.push_child_scope();
        for param in params {
            self.bind_name(
                &param.name,
                ResolvedName::Local(param.local),
                Writeability::Val,
            );
        }
        self.resolve_expr(body);
        self.pop_scope();
        let (_, _, captures) = self.closures.pop().expect("closure context");
        self.resolved.insert_closure_captures(expr_id, captures);
    }

    fn bind_pattern(&mut self, pattern: PatternId) {
        self.resolve_pattern_variants(pattern, true);
        self.bind_pattern_locals(pattern);
    }

    fn resolve_pattern_variants(&mut self, pattern: PatternId, bare_variants: bool) {
        self.assert_current_owner(pattern.owner());
        if self.cancel.check().is_err() {
            return;
        }
        match self.module.pattern(pattern).kind.clone() {
            PatternKind::Name { name, .. } if bare_variants => {
                self.record_pattern_variant(pattern, &name);
            }
            PatternKind::EnumVariant { path, fields } => {
                self.record_pattern_variant(pattern, &path);
                for field in fields {
                    self.resolve_pattern_variants(field, bare_variants);
                }
            }
            PatternKind::Or(fields) | PatternKind::Tuple(fields) => {
                for field in fields {
                    self.resolve_pattern_variants(field, bare_variants);
                }
            }
            PatternKind::Struct { fields, .. } => {
                for field in fields {
                    self.resolve_pattern_variants(field.pattern, bare_variants);
                }
            }
            _ => {}
        }
    }

    fn record_pattern_variant(&mut self, pattern: PatternId, path: &str) {
        let Some(resolved) = self.resolve_name(path, NameNamespace::Value) else {
            return;
        };
        if matches!(&resolved, ResolvedName::Source(source) if matches!(source.item, SourceItem::Variant(_)))
        {
            self.resolved.pattern_variants.insert(pattern, resolved);
        }
    }

    fn bind_pattern_locals(&mut self, pattern: PatternId) {
        match &self.module.pattern(pattern).kind {
            PatternKind::Or(alternatives) => {
                if let Some(first) = alternatives.first() {
                    self.bind_pattern_locals(*first);
                }
            }
            PatternKind::Name { name, local } if !name.is_empty() && name != "<missing>" => {
                if self.resolved.pattern_variants.contains_key(&pattern) {
                    return;
                }
                let name = name.clone();
                let local = *local;
                self.bind_name(&name, ResolvedName::Local(local), Writeability::Val);
            }
            PatternKind::Tuple(elements) => {
                let elements = elements.clone();
                for element in elements {
                    self.bind_pattern_locals(element);
                }
            }
            PatternKind::Struct { fields, .. } => {
                let fields = fields.clone();
                for field in fields {
                    self.bind_pattern_locals(field.pattern);
                }
            }
            PatternKind::EnumVariant { fields, .. } => {
                let fields = fields.clone();
                for field in fields {
                    self.bind_pattern_locals(field);
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
                if let Some(resolved) = self.resolve_name(name, NameNamespace::Value) {
                    self.resolved.insert_place(place_id, resolved.clone());
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
                captures.push(resolved.clone());
            }
        }
    }

    fn binding(&self, name: &str, namespace: NameNamespace) -> Option<NameResolution> {
        for scope in self
            .scopes
            .iter()
            .rev()
            .filter(|_| namespace == NameNamespace::Value)
        {
            if let Some(resolved) = scope.latest.get(name) {
                let owner = self.scopes.last().expect("active body").owner;
                let accessible = match resolved {
                    ResolvedName::Local(id) => id.owner() == HirOwner::Body(owner),
                    ResolvedName::Param(id) => id.owner() == HirOwner::Body(owner),
                    _ => true,
                };
                return Some(if accessible {
                    NameResolution::Unique(resolved.clone())
                } else {
                    NameResolution::Unresolved
                });
            }
        }
        self.names.lookup(name, namespace)
    }

    /// Checks lexical/module bindings, then qualified namespace lookup, then recognized value helpers; path roots select Type independently of locals.
    fn resolve_name(&self, name: &str, namespace: NameNamespace) -> Option<ResolvedName> {
        if let Some(binding) = self.binding(name, namespace) {
            return binding.target();
        }
        if let Some(hit) =
            self.resolved
                .catalog
                .resolve_name(self.names, name, namespace, &self.cancel)
        {
            return Some(hit.target.resolved(self.names.unit.as_ref()));
        }
        if namespace == NameNamespace::Value
            && let Some(helper) = BuiltinFunction::from_name(name)
        {
            return Some(ResolvedName::RuntimeHelper(helper));
        }
        None
    }

    /// Publishes one canonical binding and updates the disposable lexical frame.
    fn bind_name(&mut self, name: &str, resolved: ResolvedName, writeability: Writeability) {
        match resolved {
            ResolvedName::Param(id) => self.assert_current_owner(id.owner()),
            ResolvedName::Local(id) => self.assert_current_owner(id.owner()),
            ResolvedName::Function(_) => {}
            _ => unreachable!("only lexical declarations enter a body scope"),
        }
        if let Some(scope) = self.scopes.last_mut() {
            self.resolved.insert_binding(Binding {
                name: name.to_owned(),
                resolved: resolved.clone(),
                owner: scope.owner,
                writeability,
            });
            scope.latest.insert(name.to_owned(), resolved);
        }
    }

    fn push_scope(&mut self, owner: BodyOwner) {
        self.scopes.push(ActiveScope {
            owner,
            latest: HashMap::new(),
        });
    }

    fn push_child_scope(&mut self) {
        let owner = self.scopes.last().expect("body owns its scopes").owner;
        self.push_scope(owner);
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn assert_current_owner(&self, owner: HirOwner) {
        let current = self.scopes.last().expect("body owns its nodes").owner;
        assert_eq!(
            owner,
            HirOwner::Body(current),
            "HIR traversal crossed a body boundary"
        );
    }
}
