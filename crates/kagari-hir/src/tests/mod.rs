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

use crate::analysis::AnalysisDatabase;
use kagari_stdlib::catalog;

pub(crate) fn test_analysis() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared().unwrap());
    database
}

mod language_contracts;
