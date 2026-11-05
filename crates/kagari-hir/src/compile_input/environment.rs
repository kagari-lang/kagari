//! Declaration contracts available to elaboration and substitution-dependent planning.

use crate::{
    CheckedAnalysis,
    aggregates::AggregateCatalog,
    declarations::Declarations,
    imports::{
        constants::ImportedConstants, functions::ImportedFunctions, statics::ImportedStatics,
    },
    resolver::resolved::ResolvedName,
    semantic::signatures::BodySignatures,
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};

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
        }
    }

    /// Returns the canonical declaration selected by a resolved local/imported name.
    pub fn definition(self, name: ResolvedName) -> Option<DefinitionId> {
        self.declarations.definition(name).copied()
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
