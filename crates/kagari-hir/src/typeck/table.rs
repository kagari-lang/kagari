use std::collections::HashMap;

use crate::hir::ExprId;
use crate::types::TypeId;

#[derive(Debug, Clone, Default)]
pub struct TypeTable {
    exprs: HashMap<ExprId, TypeId>,
}

impl TypeTable {
    pub(crate) fn insert_expr(&mut self, id: ExprId, ty: TypeId) {
        self.exprs.insert(id, ty);
    }

    pub fn expr_type(&self, id: ExprId) -> Option<TypeId> {
        self.exprs.get(&id).copied()
    }
}
