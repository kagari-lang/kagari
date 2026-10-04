mod common;
mod language_conformance;
mod lower;
mod profile;
mod recovery;
mod resolver;
mod typeck;

pub(crate) mod native;

use crate::analysis::AnalysisDatabase;
use kagari_stdlib::catalog;

pub(crate) fn test_analysis() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared());
    database
}
