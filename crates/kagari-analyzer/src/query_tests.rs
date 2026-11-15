//! Analyzer-owned editor contracts; semantic preparation remains explicit.

use kagari_hir::analysis::AnalysisDatabase;

pub(crate) fn test_analysis() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(kagari_stdlib::catalog::shared().unwrap());
    database
}

mod analysis_tests;
mod associated_const;
mod callable;
mod common;
mod constructor;
mod external;
mod gat;
mod generic_type_context;
mod generic_type_recovery;
mod identity;
mod imports_aggregate;
mod imports_alias;
mod imports_cache;
mod imports_provenance;
mod imports_signature;
mod imports_tests;
mod imports_types;
mod member;
mod namespace;
mod native_facade;
mod native_path;
mod patterns;
mod payload;
mod prelude;
mod scoped_identity;
mod standard_query;
mod statics;
mod trait_catalog;
mod trait_identity;
mod trait_reference;
mod type_application;
mod type_name;

pub(crate) mod native;

mod staged;

// Share exact offline provider contracts without exposing a production test-support API.
#[path = "../../kagari-hir/tests/support/native.rs"]
pub(crate) mod native_fixture;
