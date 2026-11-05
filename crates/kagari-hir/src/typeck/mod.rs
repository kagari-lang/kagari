//! Signature checking, body inference and node-keyed semantic facts.
//!
//! The internal `check` module prepares signatures before checking selected bodies. [`table::TypeTable`]
//! records types and selected operations; [`crate::semantic::body::ModuleFacts`] retains the resulting
//! functions/constants. Declaration identities come from [`crate::declarations`],
//! and nominal/trait contracts from [`crate::aggregates`].
//!
//! ```text
//! declarations + imports -> check_signatures -> ModuleSignatures + diagnostics
//! signatures + aggregates + resolved names -> check_bodies_controlled
//!   -> inference/constraints -> selected call/member/protocol facts
//!   -> solve/seal temporary inference -> ModuleFacts + diagnostics
//! ```
//!
//! Unknown/error types keep incomplete-source queries useful. Unresolved inference
//! must not escape as executable facts. Reuse modules remap local IDs explicitly;
//! copying facts into a fresh lowering without remapping would violate arena checks.
//! Type-resolution work state is private to checking; retained tables contain no
//! annotation-hole map or active recursion guard. Extracting a table preserves
//! recovery facts and does not by itself prove compilation eligibility.

use crate::{
    AnalysisResult,
    aggregates::AggregateCatalog,
    declarations::Declarations,
    hir::{
        ids::{BodySelection, ConstId, ExprId, LocalId, ParamId, StaticId},
        item::behavior::GenericParam,
        writeability::Writeability,
    },
    imports::{
        constants::ImportedConstants, functions::ImportedFunctions, statics::ImportedStatics,
    },
    semantic::signatures::{BodySignatures, TypedFunction, TypedFunctionBuffer},
    typeck::{
        const_budget::ConstLimits, constant_facts::CheckedConstants, scalar::ScalarValue,
        table::TypeTable,
    },
    types::{NominalType, TypeId},
};

use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
pub(crate) mod aliases;
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
mod state;

pub mod reuse;
pub(crate) mod signature_reuse;
pub mod table;
mod ty;

use std::{collections::HashMap, sync::Arc};

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

/// Per-module constant type index available to body checking.
#[derive(Debug, Clone, Default)]
pub(crate) struct TopLevelTypeIndex {
    pub(crate) consts: HashMap<ConstId, TypeId>,
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
    pub(crate) signatures: &'a BodySignatures,
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
    /// Shared normalized contracts belonging to the matching semantic environment.
    pub body_signatures: &'a Arc<BodySignatures>,
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

#[cfg(test)]
mod tests;
