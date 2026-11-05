//! Declaration contracts available to elaboration and substitution-dependent planning.

use crate::{
    CheckedAnalysis,
    aggregates::AggregateCatalog,
    declarations::Declarations,
    hir::ids::ConstId,
    imports::{
        constants::ImportedConstants, functions::ImportedFunctions, statics::ImportedStatics,
    },
    resolver::resolved::ResolvedName,
    semantic::signatures::BodySignatures,
    typeck::scalar::ScalarValue,
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use std::collections::HashMap;

/// Borrowed declaration environment of one admitted module.
///
/// The fields share the checked snapshot's storage. This view exposes no body
/// node arena, inference workspace, database callback or mutable fact table.
/// Generic binders and projections retain their definition scope; instance
/// planning must use these same contracts when applying type arguments.
#[derive(Clone, Copy)]
pub struct CheckedEnvironment<'a> {
    declarations: &'a Declarations,
    signatures: &'a BodySignatures,
    aggregates: &'a AggregateCatalog,
    functions: &'a ImportedFunctions,
    statics: &'a ImportedStatics,
    constants: &'a ImportedConstants,
    local_constants: &'a HashMap<ConstId, ScalarValue>,
}

impl<'a> CheckedEnvironment<'a> {
    pub(super) fn new(module: &'a CheckedAnalysis) -> Self {
        Self {
            declarations: &module.declarations,
            signatures: module.typed.signatures(),
            aggregates: &module.aggregates,
            functions: &module.imported_functions,
            statics: &module.imported_statics,
            constants: &module.imported_constants,
            local_constants: module.typed.const_values(),
        }
    }

    /// Returns the canonical declaration selected by a resolved local/imported name.
    pub fn definition(self, name: ResolvedName) -> Option<DefinitionId> {
        self.declarations.definition(name).copied()
    }

    /// Borrows the already evaluated scalar selected by a local or imported constant binding.
    ///
    /// Import aliases select the same original fact. Missing values are not evaluated
    /// here: admission and constant checking own that work before elaboration.
    pub fn constant_value(self, binding: &ResolvedName) -> Option<&'a ScalarValue> {
        match binding {
            ResolvedName::Const(id) => self.local_constants.get(id),
            ResolvedName::Source(source) => {
                self.constants.get(source).map(|constant| &constant.value)
            }
            _ => None,
        }
    }

    /// Returns the canonical identity of a checked runtime static selected by a binding.
    ///
    /// This only projects the checked declaration/type contract. It neither allocates
    /// a runtime slot nor executes an initializer; instance planning owns that demand.
    pub fn static_declaration(self, binding: &ResolvedName) -> Option<DefinitionId> {
        match binding {
            ResolvedName::Static(id) => {
                self.signatures.static_type(*id)?;
                self.definition(binding.clone())
            }
            ResolvedName::Source(source) => self.statics.get(source).map(|item| item.declaration),
            _ => None,
        }
    }

    /// Resolves the scoped definition identities in this environment.
    pub fn definitions(self) -> &'a DefinitionTable {
        self.declarations.definitions()
    }

    /// Borrows declaration identities, binders and source sites.
    pub fn declarations(self) -> &'a Declarations {
        self.declarations
    }

    /// Borrows normalized local callable and static contracts.
    pub fn signatures(self) -> &'a BodySignatures {
        self.signatures
    }

    /// Borrows the fixed nominal/trait environment used to select implementations.
    pub fn aggregates(self) -> &'a AggregateCatalog {
        self.aggregates
    }

    /// Borrows canonical foreign callable contracts, independent of import spelling.
    pub fn imported_functions(self) -> &'a ImportedFunctions {
        self.functions
    }

    /// Borrows checked foreign static types and rebinding permissions.
    pub fn imported_statics(self) -> &'a ImportedStatics {
        self.statics
    }

    /// Borrows foreign scalar constants and their original declaration identities.
    pub fn imported_constants(self) -> &'a ImportedConstants {
        self.constants
    }
}
