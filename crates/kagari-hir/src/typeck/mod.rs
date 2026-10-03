#[cfg(test)]
use crate::hir::ids::HirArenaId;
use crate::{
    AnalysisResult,
    aggregates::AggregateCatalog,
    declarations::Declarations,
    hir::{
        ids::{BodySelection, ConstId, ExprId, FunctionId, LocalId, ParamId},
        item::behavior::GenericParam,
        writeability::Writeability,
    },
    imports::functions::ImportedFunctions,
    native::NativeBinding,
    typeck::{
        const_budget::ConstLimits,
        scalar::ScalarValue,
        table::{ConstraintTarget, TypeTable},
    },
    types::{GenericParameterType, TypeId},
};

use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};
use smallvec::SmallVec;
pub(crate) mod applications;
pub(crate) mod associated;
pub(crate) mod associated_consts;
mod body;
pub(crate) mod check;
mod completion;
pub mod const_budget;
mod const_eval;
mod families;
pub(crate) mod supertraits;

pub mod constraints;
mod inference;
pub(crate) mod members;
pub mod scalar;
mod solver;

pub mod reuse;
pub(crate) mod signature_reuse;
pub mod table;
mod ty;

use std::collections::HashMap;

pub(crate) type TypedFunctionBuffer = SmallVec<[TypedFunction; 8]>;
pub(crate) type TypedParameterBuffer = SmallVec<[TypedParameter; 4]>;
pub type GenericBounds = HashMap<TypeId, Vec<ConstraintTarget>>;

#[derive(Debug, Clone)]
pub struct ModuleSignatures {
    pub(crate) type_bounds: HashMap<DefinitionId, GenericBounds>,
    pub(crate) functions: TypedFunctionBuffer,
    pub(crate) type_table: TypeTable,
}

impl ModuleSignatures {
    pub fn type_bounds(&self, id: &DefinitionId) -> Option<&GenericBounds> {
        self.type_bounds.get(id)
    }

    #[cfg(test)]
    pub(crate) fn assert_same_source_facts(
        &self,
        other: &Self,
        arena: HirArenaId,
        other_arena: HirArenaId,
    ) {
        let mut functions = self.functions.clone();
        for function in &mut functions {
            for param in &mut function.params {
                assert_eq!(param.id.arena(), arena);
                param.id = ParamId::new(other_arena, param.id.owner(), param.id.index());
            }
        }
        for function in &other.functions {
            for param in &function.params {
                assert_eq!(param.id.arena(), other_arena);
            }
        }
        assert_eq!(functions, other.functions);
        assert_eq!(self.type_bounds, other.type_bounds);
        self.type_table
            .assert_same_source_facts(&other.type_table, arena, other_arena);
    }

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
    /// The implementation belongs to the declaration, independently of call syntax.
    pub implementation: FunctionImplementation,
    pub generic_params: Vec<GenericParameterType>,
    /// Checked constraints keyed by the declaring parameter, including inherited
    /// impl parameters shadowed by a method parameter with the same name.
    pub bounds: HashMap<TypeId, Vec<ConstraintTarget>>,
    pub id: FunctionId,
    pub name: String,
    pub params: TypedParameterBuffer,
    pub return_type: TypeId,
}

/// Implementation provenance carried with checked callable signatures.
/// The enclosing module/function identity selects a script body; required trait
/// methods await an implementation. Native bindings are installed input and do
/// not acquire authority from the declaration's name or source URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionImplementation {
    Script,
    Native(NativeBinding),
    Required,
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
    pub(crate) aggregates: &'a AggregateCatalog,
    pub(crate) imported_functions: &'a ImportedFunctions,
    pub(crate) declarations: &'a Declarations,
    pub(crate) cancel: &'a CancellationToken,
    pub(crate) function_index: &'a FunctionTypeIndex,
    pub(crate) top_level_index: &'a TopLevelTypeIndex,
    pub(crate) const_values: Option<&'a HashMap<ConstId, ScalarValue>>,
}

pub(crate) struct BodyInputs<'a> {
    pub const_limits: ConstLimits,
    pub selection: BodySelection,
    pub signatures: &'a AnalysisResult<ModuleSignatures>,
    pub imported_functions: &'a ImportedFunctions,
    pub aggregates: &'a AggregateCatalog,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BodyTypeEnv {
    pub(crate) self_type: Option<TypeId>,
    pub(crate) params: HashMap<ParamId, TypeId>,
    pub(crate) locals: HashMap<LocalId, TypeId>,
    pub(crate) local_writeability: HashMap<LocalId, Writeability>,
    pub(crate) exprs: HashMap<ExprId, TypeId>,
    pub(crate) generics: Vec<GenericParam>,
    pub(crate) generic_bounds: HashMap<TypeId, Vec<ConstraintTarget>>,
}
