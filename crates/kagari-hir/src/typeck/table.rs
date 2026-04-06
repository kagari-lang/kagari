use std::collections::HashMap;

use crate::hir::{ExprId, LocalId};
use crate::types::TypeId;

#[derive(Debug, Clone, Default)]
pub struct TypeTable {
    exprs: HashMap<ExprId, TypeId>,
    locals: HashMap<LocalId, TypeId>,
}

impl TypeTable {
    pub(crate) fn insert_expr(&mut self, id: ExprId, ty: TypeId) {
        self.exprs.insert(id, ty);
    }

    pub(crate) fn insert_local(&mut self, id: LocalId, ty: TypeId) {
        self.locals.insert(id, ty);
    }

    pub fn expr_type(&self, id: ExprId) -> Option<TypeId> {
        self.exprs.get(&id).copied()
    }

    pub fn local_type(&self, id: LocalId) -> Option<TypeId> {
        self.locals.get(&id).copied()
    }
}
