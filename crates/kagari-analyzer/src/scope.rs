//! Demand-built lexical visibility over structural HIR and canonical binding identities.

use kagari_common::span::Span;
use kagari_hir::{
    hir::{
        expr::{Condition, ExprKind},
        ids::{BlockId, BodyOwner, ExprId, FunctionId, HirOwner, PatternId, PlaceId, StmtId},
        item::function::FunctionKind,
        place::PlaceKind,
        stmt::StmtKind,
    },
    lower::LoweredModule,
    resolver::resolved::{Binding, ResolvedName, ResolvedNames},
};
use std::{cmp::Reverse, collections::HashSet};

#[derive(Debug)]
struct Scope<'a> {
    owner: BodyOwner,
    span: Span,
    // Freeze the parent's declaration prefix at entry, especially for named local functions.
    parent: Option<(usize, usize)>,
    bindings: Vec<(&'a Binding, usize)>,
}

/// Query-local index borrowing the exact lowering and resolution that own its entries.
/// It projects structure and selected identities; it never resolves a spelling or variant.
#[derive(Debug)]
pub(crate) struct ScopeIndex<'a> {
    lowered: &'a LoweredModule,
    names: &'a ResolvedNames,
    scopes: Vec<Scope<'a>>,
}

impl<'a> ScopeIndex<'a> {
    pub(crate) fn new(lowered: &'a LoweredModule, names: &'a ResolvedNames) -> Self {
        let mut index = Self {
            lowered,
            names,
            scopes: Vec::new(),
        };
        for function in &lowered.module.functions {
            if !matches!(function.kind, FunctionKind::Local { .. }) {
                index.function(function.id, None);
            }
        }
        for constant in &lowered.module.consts {
            let scope = index.scope(
                BodyOwner::Const(constant.id),
                lowered.source_map.expr_span(constant.initializer),
                None,
            );
            index.expr(constant.initializer, scope);
        }
        for item in &lowered.module.statics {
            let scope = index.scope(
                BodyOwner::Static(item.id),
                lowered.source_map.expr_span(item.initializer),
                None,
            );
            index.expr(item.initializer, scope);
        }
        index
    }

    pub(crate) fn visible_bindings(&self, offset: usize) -> Vec<&'a Binding> {
        let Some((id, selected)) = self
            .scopes
            .iter()
            .enumerate()
            .filter(|(_, scope)| scope.span.start <= offset && offset < scope.span.end)
            .min_by_key(|(id, scope)| (scope.span.end - scope.span.start, Reverse(*id)))
        else {
            return Vec::new();
        };
        let owner = selected.owner;
        let mut current = Some((id, selected.bindings.len()));
        let mut seen = HashSet::new();
        let mut visible = Vec::new();
        while let Some((id, limit)) = current {
            let scope = &self.scopes[id];
            for (binding, start) in scope.bindings[..limit].iter().rev() {
                if *start <= offset && seen.insert(binding.name.as_str()) {
                    let accessible = match binding.resolved {
                        ResolvedName::Local(id) => id.owner() == HirOwner::Body(owner),
                        ResolvedName::Param(id) => id.owner() == HirOwner::Body(owner),
                        _ => true,
                    };
                    if accessible {
                        visible.push(*binding);
                    }
                }
            }
            current = scope.parent;
        }
        visible.sort_by(|a, b| a.name.cmp(&b.name));
        visible
    }

    fn scope(&mut self, owner: BodyOwner, span: Span, parent: Option<usize>) -> usize {
        let parent = parent.map(|id| (id, self.scopes[id].bindings.len()));
        let id = self.scopes.len();
        self.scopes.push(Scope {
            owner,
            span,
            parent,
            bindings: Vec::new(),
        });
        id
    }

    fn child(&mut self, parent: usize, span: Span) -> usize {
        self.scope(self.scopes[parent].owner, span, Some(parent))
    }

    fn binding(&mut self, scope: usize, name: ResolvedName, start: usize) {
        if let Some(binding) = self.names.binding(&name) {
            assert_eq!(
                binding.owner, self.scopes[scope].owner,
                "binding index owner mismatch"
            );
            self.scopes[scope].bindings.push((binding, start));
        }
    }

    fn pattern(&mut self, scope: usize, pattern: PatternId, start: usize) {
        for (local, _) in self.lowered.module.body.pattern_bindings(pattern) {
            self.binding(scope, ResolvedName::Local(local), start);
        }
    }

    fn function(&mut self, id: FunctionId, parent: Option<usize>) {
        let function = &self.lowered.module.functions[id.index()];
        let Some(body) = function.body else {
            return;
        };
        let scope = self.scope(
            BodyOwner::Function(id),
            self.lowered.source_map.function_span(id),
            parent,
        );
        for param in &function.params {
            self.binding(
                scope,
                ResolvedName::Param(param.id),
                self.lowered.source_map.block_span(body).start,
            );
        }
        self.block(body, scope);
    }

    fn block(&mut self, id: BlockId, parent: usize) {
        let block = self.lowered.module.block(id);
        let span = self.lowered.source_map.block_span(id);
        let scope = self.child(parent, span);
        // Local function names are hoisted, but their non-capturing environments
        // see only the lexical prefix at their own declaration.
        for stmt in &block.statements {
            if let StmtKind::Function(id) = self.lowered.module.stmt(*stmt).kind {
                self.binding(scope, ResolvedName::Function(id), span.start);
            }
        }
        for stmt in &block.statements {
            self.stmt(*stmt, scope);
        }
        if let Some(expr) = block.tail_expr {
            self.expr(expr, scope);
        }
    }

    fn condition_block(&mut self, condition: &Condition, body: BlockId, scope: usize) {
        self.expr(condition.value(), scope);
        let scope = if let Condition::Binding { pattern, .. } = condition {
            let span = self.lowered.source_map.block_span(body);
            let child = self.child(scope, span);
            self.pattern(child, *pattern, span.start);
            child
        } else {
            scope
        };
        self.block(body, scope);
    }

    fn stmt(&mut self, id: StmtId, scope: usize) {
        match &self.lowered.module.stmt(id).kind {
            StmtKind::Binding {
                pattern,
                initializer,
                ..
            } => {
                self.expr(*initializer, scope);
                self.pattern(scope, *pattern, self.lowered.source_map.stmt_span(id).end);
            }
            StmtKind::Assign { target, value, .. } => {
                self.place(*target, scope);
                self.expr(*value, scope);
            }
            StmtKind::Return { expr } => {
                if let Some(expr) = expr {
                    self.expr(*expr, scope);
                }
            }
            StmtKind::While { condition, body } => self.condition_block(condition, *body, scope),
            StmtKind::Loop { body } => self.block(*body, scope),
            StmtKind::For {
                pattern,
                iterable,
                body,
            } => {
                self.expr(*iterable, scope);
                let span = self.lowered.source_map.block_span(*body);
                let child = self.child(scope, span);
                self.pattern(child, *pattern, span.start);
                self.block(*body, child);
            }
            StmtKind::Expr(expr) | StmtKind::BreakValue(expr) => self.expr(*expr, scope),
            StmtKind::Function(id) => self.function(*id, Some(scope)),
            StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn expr(&mut self, id: ExprId, scope: usize) {
        match &self.lowered.module.expr(id).kind {
            ExprKind::Missing | ExprKind::Name { .. } | ExprKind::Literal(_) => {}
            ExprKind::FormatPart { expr, .. }
            | ExprKind::Await { expr }
            | ExprKind::Propagate { expr }
            | ExprKind::Cast { expr, .. }
            | ExprKind::Prefix { expr, .. } => self.expr(*expr, scope),
            ExprKind::Binary { lhs, rhs, .. }
            | ExprKind::ArrayRepeat {
                value: lhs,
                count: rhs,
            } => {
                self.expr(*lhs, scope);
                self.expr(*rhs, scope);
            }
            ExprKind::Range { start, end, .. } => {
                for expr in start.iter().chain(end) {
                    self.expr(*expr, scope);
                }
            }
            ExprKind::Call { callee, args, .. } => {
                self.expr(*callee, scope);
                for arg in args {
                    self.expr(*arg, scope);
                }
            }
            ExprKind::Field { receiver, .. } => self.expr(*receiver, scope),
            ExprKind::Index { receiver, index } => {
                self.expr(*receiver, scope);
                self.expr(*index, scope);
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.condition_block(condition, *then_branch, scope);
                if let Some(expr) = else_branch {
                    self.expr(*expr, scope);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(*scrutinee, scope);
                for arm in arms {
                    let mut span = self.lowered.source_map.expr_span(arm.expr);
                    if let Some(guard) = arm.guard {
                        span.start = self.lowered.source_map.expr_span(guard).start;
                    }
                    let child = self.child(scope, span);
                    self.pattern(child, arm.pattern, span.start);
                    if let Some(guard) = arm.guard {
                        self.expr(guard, child);
                    }
                    self.expr(arm.expr, child);
                }
            }
            ExprKind::StructInit { fields, base, .. } => {
                for field in fields {
                    self.expr(field.value, scope);
                }
                if let Some(base) = base {
                    self.expr(*base, scope);
                }
            }
            ExprKind::InterpolatedString(elements)
            | ExprKind::Tuple(elements)
            | ExprKind::Array { elements, .. } => {
                for expr in elements {
                    self.expr(*expr, scope);
                }
            }
            ExprKind::Block(block) | ExprKind::Loop { body: block } => self.block(*block, scope),
            ExprKind::Closure { params, body, .. } => {
                let child = self.child(scope, self.lowered.source_map.expr_span(id));
                for param in params {
                    self.binding(
                        child,
                        ResolvedName::Local(param.local),
                        self.lowered.source_map.expr_span(*body).start,
                    );
                }
                self.expr(*body, child);
            }
            ExprKind::AsyncBlock { body } => {
                let child = self.child(scope, self.lowered.source_map.expr_span(id));
                self.expr(*body, child);
            }
        }
    }

    fn place(&mut self, id: PlaceId, scope: usize) {
        match self.lowered.module.place(id).kind {
            PlaceKind::Name(_) => {}
            PlaceKind::Expr(expr) => self.expr(expr, scope),
            PlaceKind::Field { base, .. } => self.place(base, scope),
            PlaceKind::Index { base, index } => {
                self.place(base, scope);
                self.expr(index, scope);
            }
        }
    }
}
