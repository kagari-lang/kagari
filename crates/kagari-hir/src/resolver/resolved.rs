use std::collections::HashMap;

use crate::hir::{
    ConstId, EnumId, ExprId, FunctionId, LocalId, ParamId, PlaceId, StaticId, StructId,
};
use crate::resolver::table::NameTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedName {
    Function(FunctionId),
    Const(ConstId),
    Param(ParamId),
    Local(LocalId),
    Static(StaticId),
    Struct(StructId),
    Enum(EnumId),
}

#[derive(Debug, Clone)]
pub struct ResolvedNames {
    pub items: NameTable,
    exprs: HashMap<ExprId, ResolvedName>,
    places: HashMap<PlaceId, ResolvedName>,
}

impl ResolvedNames {
    pub(crate) fn new(items: NameTable) -> Self {
        Self {
            items,
            exprs: HashMap::new(),
            places: HashMap::new(),
        }
    }

    pub(crate) fn insert_expr(&mut self, id: ExprId, resolved: ResolvedName) {
        self.exprs.insert(id, resolved);
    }

    pub(crate) fn insert_place(&mut self, id: PlaceId, resolved: ResolvedName) {
        self.places.insert(id, resolved);
    }

    pub fn expr_resolution(&self, id: ExprId) -> Option<ResolvedName> {
        self.exprs.get(&id).copied()
    }

    pub fn place_resolution(&self, id: PlaceId) -> Option<ResolvedName> {
        self.places.get(&id).copied()
    }
}
