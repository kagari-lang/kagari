mod body;
mod check;
mod const_eval;
mod constraints;
mod inference;
mod scalar;
pub use scalar::ScalarValue;
mod reuse;
mod signature_reuse;
pub(crate) use signature_reuse::reuse_signatures;
mod table;
mod ty;
pub use reuse::BodyReuse;

use crate::types::TypeId;
use std::collections::HashMap;

use crate::hir::{ConstId, ExprId, FunctionId, LocalId, ParamId, Writeability};

pub(crate) type TypedFunctionBuffer = smallvec::SmallVec<[TypedFunction; 8]>;
pub(crate) type TypedParameterBuffer = smallvec::SmallVec<[TypedParameter; 4]>;

pub(crate) use check::{check_bodies_controlled, check_signatures};
pub use table::{
    CallTarget, ConstraintTarget, ResolvedCall, ResolvedStructInit, ResolvedTypeRef, TypeTable,
    TypeTarget,
};

#[derive(Debug, Clone)]
pub struct ModuleSignatures {
    pub(crate) functions: TypedFunctionBuffer,
    pub(crate) type_table: TypeTable,
}

impl ModuleSignatures {
    pub fn functions(&self) -> &[TypedFunction] {
        &self.functions
    }
    pub fn type_table(&self) -> &TypeTable {
        &self.type_table
    }
}

#[derive(Debug, Clone)]
pub struct TypedModule {
    pub checked_bodies: usize,
    pub reused_bodies: usize,
    pub functions: TypedFunctionBuffer,
    pub consts: HashMap<ConstId, TypeId>,
    pub const_values: HashMap<ConstId, ScalarValue>,
    pub type_table: TypeTable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedFunction {
    pub generic_params: Vec<crate::types::GenericParameterType>,
    pub id: FunctionId,
    pub name: String,
    pub params: TypedParameterBuffer,
    pub return_type: TypeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedParameter {
    pub id: ParamId,
    pub writeability: Writeability,
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
}

#[derive(Clone, Copy)]
pub(crate) struct TypeIndexes<'a> {
    pub(crate) aggregates: &'a crate::aggregates::AggregateCatalog,
    pub(crate) imported_functions: &'a crate::imports::ImportedFunctions,
    pub(crate) declarations: &'a crate::declarations::Declarations,
    pub(crate) cancel: &'a kagari_common::cancellation::CancellationToken,
    pub(crate) function_index: &'a FunctionTypeIndex,
    pub(crate) top_level_index: &'a TopLevelTypeIndex,
}

pub(crate) struct BodyInputs<'a> {
    pub selection: crate::hir::BodySelection,
    pub signatures: &'a crate::AnalysisResult<ModuleSignatures>,
    pub imported_functions: &'a crate::imports::ImportedFunctions,
    pub aggregates: &'a crate::aggregates::AggregateCatalog,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BodyTypeEnv {
    pub(crate) params: HashMap<ParamId, TypeId>,
    pub(crate) locals: HashMap<LocalId, TypeId>,
    pub(crate) local_writeability: HashMap<LocalId, Writeability>,
    pub(crate) exprs: HashMap<ExprId, TypeId>,
    pub(crate) generics: Vec<crate::hir::GenericParam>,
    pub(crate) generic_bounds: HashMap<crate::types::GenericParameterType, Vec<ConstraintTarget>>,
}
