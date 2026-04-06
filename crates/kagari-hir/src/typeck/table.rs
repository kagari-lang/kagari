use std::collections::HashMap;

use crate::hir::{ExprId, LocalId, PlaceId};
use crate::types::TypeId;

#[derive(Debug, Clone, Default)]
pub struct TypeTable {
    exprs: HashMap<ExprId, TypeId>,
    locals: HashMap<LocalId, TypeId>,
    places: HashMap<PlaceId, TypeId>,
}

impl TypeTable {
    pub(crate) fn insert_expr(&mut self, id: ExprId, ty: TypeId) {
        self.exprs.insert(id, ty);
    }

    pub(crate) fn insert_local(&mut self, id: LocalId, ty: TypeId) {
        self.locals.insert(id, ty);
    }

    pub(crate) fn insert_place(&mut self, id: PlaceId, ty: TypeId) {
        self.places.insert(id, ty);
    }

    pub fn expr_type(&self, id: ExprId) -> Option<TypeId> {
        self.exprs.get(&id).cloned()
    }

    pub fn local_type(&self, id: LocalId) -> Option<TypeId> {
        self.locals.get(&id).cloned()
    }

    pub fn place_type(&self, id: PlaceId) -> Option<TypeId> {
        self.places.get(&id).cloned()
    }
}
