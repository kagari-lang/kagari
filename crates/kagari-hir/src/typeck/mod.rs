mod body;
mod check;
mod table;
mod ty;

use crate::types::TypeId;
use std::collections::HashMap;

use crate::hir::{ConstId, ExprId, FunctionId, LocalId, ParamId, StaticId};

pub(crate) type TypedFunctionBuffer = smallvec::SmallVec<[TypedFunction; 8]>;
pub(crate) type TypedParameterBuffer = smallvec::SmallVec<[TypedParameter; 4]>;

pub use check::check_module;
pub use table::TypeTable;

#[derive(Debug, Clone)]
pub struct TypedModule {
    pub functions: TypedFunctionBuffer,
    pub consts: HashMap<ConstId, TypeId>,
    pub statics: HashMap<StaticId, TypedStatic>,
    pub type_table: TypeTable,
}

#[derive(Debug, Clone)]
pub struct TypedFunction {
    pub id: FunctionId,
    pub name: String,
    pub params: TypedParameterBuffer,
    pub return_type: TypeId,
}

#[derive(Debug, Clone)]
pub struct TypedStatic {
    pub ty: TypeId,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct TypedParameter {
    pub id: ParamId,
    pub name: String,
    pub ty: TypeId,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FunctionTypeIndex {
    pub(crate) by_id: HashMap<FunctionId, TypedFunction>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TopLevelTypeIndex {
    pub(crate) consts: HashMap<ConstId, TypeId>,
    pub(crate) statics: HashMap<StaticId, TypedStatic>,
}

#[derive(Clone, Copy)]
pub(crate) struct TypeIndexes<'a> {
    pub(crate) function_index: &'a FunctionTypeIndex,
    pub(crate) top_level_index: &'a TopLevelTypeIndex,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BodyTypeEnv {
    pub(crate) params: HashMap<ParamId, TypeId>,
    pub(crate) locals: HashMap<LocalId, TypeId>,
    pub(crate) local_mutability: HashMap<LocalId, bool>,
    pub(crate) exprs: HashMap<ExprId, TypeId>,
}
