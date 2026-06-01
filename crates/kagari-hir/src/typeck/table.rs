use std::collections::HashMap;

use crate::builtin::surface::StandardIntrinsic;
use crate::hir::{ExprId, LocalId, PlaceId};
use crate::types::TypeId;

#[derive(Debug, Clone, Default)]
pub struct TypeTable {
    exprs: HashMap<ExprId, TypeId>,
    locals: HashMap<LocalId, TypeId>,
    places: HashMap<PlaceId, TypeId>,
    standard_calls: HashMap<ExprId, StandardIntrinsic>,
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

    pub(crate) fn insert_standard_call(&mut self, id: ExprId, intrinsic: StandardIntrinsic) {
        self.standard_calls.insert(id, intrinsic);
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

    pub fn standard_call_intrinsic(&self, id: ExprId) -> Option<StandardIntrinsic> {
        self.standard_calls.get(&id).copied()
    }
}
