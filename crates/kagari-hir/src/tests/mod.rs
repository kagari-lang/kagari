pub(crate) mod common;
mod language_conformance;
mod local_functions;
mod lower;
mod patterns;
mod recovery;
mod resolver;
mod text_literals;
mod typeck;

pub(crate) mod native;

use crate::analysis::AnalysisDatabase;
use kagari_stdlib::catalog;

pub(crate) fn test_analysis() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared().unwrap());
    database
}

mod language_contracts;
