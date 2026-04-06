use std::collections::HashMap;

use crate::hir::{
    BlockId, EnumId, ExprId, ExprKind, FunctionId, Module, ParamId, PatternKind, PlaceId,
    PlaceKind, StmtId, StmtKind, StructId,
};
use crate::resolver::{ResolvedName, ResolvedNames, table::NameTable};

pub(crate) struct BodyResolver<'a> {
    names: &'a NameTable,
    module: &'a Module,
    resolved: ResolvedNames,
    scopes: Vec<HashMap<String, ResolvedName>>,
}

impl<'a> BodyResolver<'a> {
    pub(crate) fn new(names: &'a NameTable, module: &'a Module) -> Self {
        Self {
            names,
            module,
            resolved: ResolvedNames::new(names.clone()),
            scopes: Vec::new(),
        }
    }

    pub(crate) fn finish(self) -> ResolvedNames {
        self.resolved
    }

    pub(crate) fn resolve_function(
        &mut self,
        params: impl Iterator<Item = (&'a str, ParamId)>,
        body: BlockId,
    ) {
        self.push_scope();
        for (name, id) in params {
            self.bind_name(name, ResolvedName::Param(id));
        }
        self.resolve_block(body);
        self.pop_scope();
    }

    fn resolve_block(&mut self, block_id: BlockId) {
        let block = self.module.block(block_id);
        self.push_scope();
        for stmt in &block.statements {
            self.resolve_stmt(*stmt);
        }
        if let Some(expr) = block.tail_expr {
            self.resolve_expr(expr);
        }
        self.pop_scope();
    }

    fn resolve_stmt(&mut self, stmt_id: StmtId) {
        let stmt = self.module.stmt(stmt_id);
        match &stmt.kind {
            StmtKind::Let {
                local,
                name,
                initializer,
                ..
            } => {
                self.resolve_expr(*initializer);
                if !name.is_empty() {
                    self.bind_name(name, ResolvedName::Local(*local));
                }
            }
            StmtKind::Assign { target, value } => {
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
        let expr = self.module.expr(expr_id);
        match &expr.kind {
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
                    self.push_scope();
                    if let PatternKind::Name { name, local } =
                        &self.module.pattern(arm.pattern).kind
                        && !name.is_empty()
                        && name != "<missing>"
                    {
                        self.bind_name(name, ResolvedName::Local(*local));
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
        let place = self.module.place(place_id);
        match &place.kind {
            PlaceKind::Name(name) => {
                if let Some(resolved) = self.resolve_name(name) {
                    self.resolved.insert_place(place_id, resolved);
                }
            }
        }
    }

    fn resolve_name(&self, name: &str) -> Option<ResolvedName> {
        for scope in self.scopes.iter().rev() {
            if let Some(resolved) = scope.get(name) {
                return Some(*resolved);
            }
        }

        if let Some(id) = self.names.function(name) {
            return Some(ResolvedName::Function(id));
        }
        if let Some(id) = self.names.struct_(name) {
            return Some(ResolvedName::Struct(id));
        }
        self.names.enum_(name).map(ResolvedName::Enum)
    }

    fn bind_name(&mut self, name: &str, resolved: ResolvedName) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), resolved);
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
}

trait TopLevelLookup {
    fn function(&self, name: &str) -> Option<FunctionId>;
    fn struct_(&self, name: &str) -> Option<StructId>;
    fn enum_(&self, name: &str) -> Option<EnumId>;
}

impl TopLevelLookup for NameTable {
    fn function(&self, name: &str) -> Option<FunctionId> {
        self.functions.get(name).copied()
    }

    fn struct_(&self, name: &str) -> Option<StructId> {
        self.structs.get(name).copied()
    }

    fn enum_(&self, name: &str) -> Option<EnumId> {
        self.enums.get(name).copied()
    }
}
