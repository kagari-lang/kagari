pub(crate) mod cancellation;
pub(crate) mod common;
mod local_functions;
mod lower;
mod patterns;
mod recovery;
mod resolver;
#[path = "../tests/support/standard.rs"]
pub(crate) mod standard;
mod text_literals;
mod typeck;
mod ui;

// Source owns the strict schema/projection shared with syntax, without a
// production test-serialization API or an extra test-support crate.
#[path = "../../kagari-source/tests/support/diagnostics.rs"]
mod ui_diagnostics;

pub(crate) mod native;

use crate::analysis::AnalysisDatabase;
use crate::tests::standard::standard_modules;

pub(crate) fn test_analysis() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(standard_modules());
    database
}

mod language_contracts;

// Shared declaration-only preparation also serves analyzer tests; it has no HIR dependencies.
#[path = "../tests/support/native.rs"]
pub(crate) mod native_fixture;
