//! Signature checking, body inference and node-keyed semantic facts.
//!
//! The internal `check` module prepares signatures before checking selected bodies. [`table::TypeTable`]
//! records types and selected operations; [`TypedModule`] retains the resulting
//! functions/constants. Declaration identities come from [`crate::declarations`],
//! and nominal/trait contracts from [`crate::aggregates`].
//!
//! ```text
//! declarations + imports -> check_signatures -> ModuleSignatures + diagnostics
//! signatures + aggregates + resolved names -> check_bodies_controlled
//!   -> inference/constraints -> selected call/member/protocol facts
//!   -> solve/seal temporary inference -> TypedModule + diagnostics
//! ```
//!
//! Unknown/error types keep incomplete-source queries useful. Unresolved inference
//! must not escape as executable facts. Reuse modules remap local IDs explicitly;
//! copying facts into a fresh lowering without remapping would violate arena checks.

use crate::{
    AnalysisResult,
    aggregates::AggregateCatalog,
    declarations::Declarations,
    hir::{
        ids::{BodySelection, ConstId, ExprId, FunctionId, LocalId, ParamId, StaticId},
        item::behavior::GenericParam,
        writeability::Writeability,
    },
    imports::{
        constants::ImportedConstants, functions::ImportedFunctions, statics::ImportedStatics,
    },
    native::NativeBinding,
    typeck::{
        const_budget::ConstLimits, constant_facts::CheckedConstants, scalar::ScalarValue,
        table::TypeTable,
    },
    types::{GenericParameterType, NominalType, TypeId},
};
#[cfg(test)]
use crate::{hir::ids::HirArenaId, tests::common::import_facts};
#[cfg(test)]
use kagari_common::identity::{map::DefinitionContext, table::DefinitionTable};

use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use smallvec::SmallVec;
pub(crate) mod applications;
pub(crate) mod associated;
pub(crate) mod associated_consts;
mod asynchronous;
mod body;
pub(crate) mod check;
mod completion;
pub mod const_budget;
mod const_eval;
pub(crate) mod constant_facts;
pub(crate) mod definition_registration;
mod families;
mod interfaces;
pub(crate) mod supertraits;

pub mod constraints;
pub(crate) mod inference;
pub(crate) mod members;
pub mod scalar;
mod solver;

pub mod reuse;
pub(crate) mod signature_reuse;
pub mod table;
mod ty;

use std::collections::{HashMap, HashSet};

/// Checked function records with eight inline slots before heap allocation.
pub(crate) type TypedFunctionBuffer = SmallVec<[TypedFunction; 8]>;

/// Checked parameters with four inline slots before heap allocation.
pub(crate) type TypedParameterBuffer = SmallVec<[TypedParameter; 4]>;

/// Semantic subject type to required applied traits; keys retain binder identity.
pub type GenericBounds = HashMap<TypeId, Vec<NominalType>>;

/// Reusable signature-stage facts before checking function bodies.
///
/// Contains typed callable contracts, nominal-owner bounds and type-syntax/field
/// facts. Bodies are not stored here. Declaration/aggregate preparation supplies
/// the names and trait environment; body checking consumes these same contracts.
#[derive(Debug, Clone)]
pub struct ModuleSignatures {
    /// Nominal definition identity to its checked binder constraints.
    pub(crate) type_bounds: HashMap<DefinitionId, GenericBounds>,
    /// Callable contracts in the module's prepared function order.
    pub(crate) functions: TypedFunctionBuffer,
    /// Explicit runtime static types, independent of initializer checking.
    pub(crate) statics: HashMap<StaticId, TypeId>,
    /// Signature type-reference, field and constraint facts.
    pub(crate) type_table: TypeTable,
}

impl ModuleSignatures {
    #[cfg(test)]
    pub(crate) fn assert_same_source_facts(
        &self,
        definitions: &DefinitionTable,
        other: &Self,
        other_definitions: &DefinitionTable,
        arena: HirArenaId,
        other_arena: HirArenaId,
    ) {
        let context = DefinitionContext::new().unwrap();
        let left = import_facts(self, definitions, &context);
        let right = import_facts(other, other_definitions, &context);
        let this = left.records();
        let other = right.records();
        let mut functions = this.functions.clone();
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
        assert_eq!(this.type_bounds, other.type_bounds);
        assert_eq!(this.statics, other.statics);
        this.type_table.assert_same_source_facts(
            left.definitions(),
            &other.type_table,
            right.definitions(),
            arena,
            other_arena,
        );
    }
}

/// Signature and body facts produced by type checking, alongside external diagnostics.
///
/// Counters distinguish freshly checked from reused function bodies. Error types
/// can remain in a recovered result; [`crate::AnalysisResult`] owns diagnostics and
/// the checked conversion gate. The table is not embedded into each HIR node.
#[derive(Debug, Clone)]
pub struct TypedModule {
    /// Bodies completed without errors, recorded before presentation truncation.
    pub(crate) valid_bodies: HashSet<FunctionId>,
    /// Initializers completed without errors, before diagnostic truncation.
    pub(crate) valid_static_initializers: HashSet<StaticId>,
    /// Number of function bodies freshly checked for this result.
    pub checked_bodies: usize,
    /// Number of function bodies restored from compatible prior facts.
    pub reused_bodies: usize,
    /// Typed function signatures retained for consumers.
    pub functions: TypedFunctionBuffer,
    /// Local constant IDs to inferred/declared semantic types.
    pub consts: HashMap<ConstId, TypeId>,
    /// Runtime static IDs to their declared closed semantic types.
    pub statics: HashMap<StaticId, TypeId>,
    /// Successfully evaluated scalar constants; absent entries have no published scalar value.
    pub const_values: HashMap<ConstId, ScalarValue>,
    /// Node-keyed types and selected call/member/protocol facts.
    pub type_table: TypeTable,
}

/// A declared callable's semantic signature, separate from its lowered body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedFunction {
    /// The implementation belongs to the declaration, independently of call syntax.
    pub implementation: FunctionImplementation,
    /// Generic binders, including inherited implementation binders where applicable.
    pub generic_params: Vec<GenericParameterType>,
    /// Checked constraints keyed by the declaring parameter, including inherited
    /// impl parameters shadowed by a method parameter with the same name.
    pub bounds: HashMap<TypeId, Vec<NominalType>>,
    /// Local function slot in the matching lowered module.
    pub id: FunctionId,
    /// Diagnostic name of the function/method.
    pub name: String,
    /// Named typed parameters in call order.
    pub params: TypedParameterBuffer,
    /// Declared/inferred result; recovery can retain an error type.
    pub return_type: TypeId,
}

/// Implementation provenance carried with checked callable signatures.
/// The enclosing module/function identity selects a script body; required trait
/// methods await an implementation. Native bindings are installed input and do
/// not acquire authority from the declaration's name or source URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionImplementation {
    /// A source body selected through the enclosing function/module identity.
    Script,
    /// An installed native binding carrying its registered contract.
    Native(NativeBinding),
    /// A trait requirement awaiting an implementation, with no script body.
    Required,
}

/// A function parameter binding paired with its semantic type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedParameter {
    /// Source parameter identity belonging to the same lowering.
    pub id: ParamId,
    /// Declared parameter binding writeability.
    pub writeability: Writeability,
    /// Diagnostic parameter name.
    pub name: String,
    /// Resolved semantic type, possibly recovered after an error.
    pub ty: TypeId,
}

/// Per-module function signature index used while checking calls.
#[derive(Debug, Clone, Default)]
pub(crate) struct FunctionTypeIndex {
    pub(crate) by_id: HashMap<FunctionId, TypedFunction>,
}

/// Per-module constant type index available to body checking.
#[derive(Debug, Clone, Default)]
pub(crate) struct TopLevelTypeIndex {
    pub(crate) consts: HashMap<ConstId, TypeId>,
    pub(crate) statics: HashMap<StaticId, TypeId>,
    pub(crate) imported_statics: ImportedStatics,
    pub(crate) imported_constants: ImportedConstants,
}

/// Borrowed semantic catalogs and lookup indexes shared by a body checker.
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

/// Prepared signature/import/aggregate inputs and policy for selected-body checking.
pub(crate) struct BodyInputs<'a> {
    pub const_limits: ConstLimits,
    /// All function bodies or one selected function; constants remain prerequisites.
    pub selection: BodySelection,
    /// Checked signature facts that seed body analysis.
    pub signatures: &'a AnalysisResult<ModuleSignatures>,
    /// Canonical imported callable signatures available to this module.
    pub imported_functions: &'a ImportedFunctions,
    /// Canonical runtime static declarations available to this module.
    pub imported_statics: &'a ImportedStatics,
    /// Checked scalar constants selected through normal source imports.
    pub imported_constants: &'a ImportedConstants,
    /// Source-matched constant prerequisites, prepared once for the body environment.
    pub constants: Option<&'a CheckedConstants>,
    /// Visible nominal, trait and implementation contracts.
    pub aggregates: &'a AggregateCatalog,
}

/// Mutable per-body types and generic assumptions used while checking expressions and bindings.
#[derive(Debug, Clone, Default)]
pub(crate) struct BodyTypeEnv {
    pub(crate) self_type: Option<TypeId>,
    /// Parameter IDs mapped to types in the current function or closure environment.
    pub(crate) params: HashMap<ParamId, TypeId>,
    /// Local binding IDs mapped to inferred/declared types.
    pub(crate) locals: HashMap<LocalId, TypeId>,
    /// Writeability of local bindings, independent of their value types.
    pub(crate) local_writeability: HashMap<LocalId, Writeability>,
    /// Previously inferred expression types in the current body environment.
    pub(crate) exprs: HashMap<ExprId, TypeId>,
    pub(crate) generics: Vec<GenericParam>,
    /// Assumed constraints for generic parameters and associated projections.
    pub(crate) generic_bounds: HashMap<TypeId, Vec<NominalType>>,
}

mod mapping;

impl ModuleSignatures {
    /// Borrows constraints for a known nominal owner, or `None` if absent.
    pub fn type_bounds(&self, id: &DefinitionId) -> Option<&GenericBounds> {
        self.type_bounds.get(id)
    }

    /// Borrows prepared callable contracts, including required/native declarations.
    pub fn functions(&self) -> &[TypedFunction] {
        &self.functions
    }

    /// Borrows explicit static declaration types without checking or executing initializers.
    pub fn statics(&self) -> &HashMap<StaticId, TypeId> {
        &self.statics
    }

    /// Borrows signature-level semantic facts; no body traversal is triggered.
    pub fn type_table(&self) -> &TypeTable {
        &self.type_table
    }
}
