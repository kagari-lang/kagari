pub(crate) mod common;
mod language_conformance;
mod local_functions;
mod lower;
mod patterns;
mod recovery;
mod resolver;
mod text_literals;
mod typeck;
mod ui;

// Source owns the strict schema/projection shared with syntax, without a
// production test-serialization API or an extra test-support crate.
#[path = "../../../kagari-source/tests/support/diagnostics.rs"]
mod ui_diagnostics;

pub(crate) mod native;

use crate::{
    analysis::AnalysisDatabase,
    types::{NominalType, TypeId},
};
use kagari_common::identity::{DefinitionKind, table::DefinitionTable};
use kagari_stdlib::catalog;

pub(crate) fn test_analysis() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared().unwrap());
    database
}

mod language_contracts;

/// Build the ordinary installed nominal application used by collection fixtures.
pub(crate) fn native_type(
    definitions: &DefinitionTable,
    name: &str,
    arguments: Vec<TypeId>,
) -> TypeId {
    TypeId::NativeObject(NominalType {
        declaration: definitions
            .lookup(&kagari_stdlib::identity::type_id(
                name,
                DefinitionKind::AssociatedType,
            ))
            .unwrap(),
        arguments,
        associated_types: Default::default(),
    })
}
