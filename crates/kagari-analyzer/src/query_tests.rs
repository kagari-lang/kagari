//! Analyzer-owned editor contracts; semantic preparation remains explicit.

use kagari::engine::KagariEngine;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_hir::{
    analysis::AnalysisDatabase,
    native::render::{DeclarationSource, declaration_source},
};
use kagari_source::source_database::{CapturedSource, SourceDatabase};
use kagari_stdlib::StandardLibrary;
use kagari_types::declaration::module::ModuleDecl;
use std::sync::{Arc, LazyLock};

pub(crate) fn standard_modules() -> Vec<Arc<DefinitionMetadata<ModuleDecl>>> {
    static MODULES: LazyLock<Vec<Arc<DefinitionMetadata<ModuleDecl>>>> = LazyLock::new(|| {
        let builder = KagariEngine::builder().unwrap();
        let standard = StandardLibrary::new(builder.declarations()).unwrap();
        standard.declarations().declarations().to_vec()
    });
    MODULES.clone()
}

pub(crate) fn render_native(
    module: &DefinitionMetadata<ModuleDecl>,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
) -> DeclarationSource {
    declaration_source(module, providers).unwrap()
}

/// Publish with the real project importers; the querying database gets only documents and scopes.
pub(crate) fn publish_native_documents(
    sources: &mut SourceDatabase,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
) -> (AnalysisDatabase, Vec<DeclarationSource>) {
    let cancel = Default::default();
    let mut publisher = AnalysisDatabase::default();
    publisher.set_native_modules(providers.to_vec());
    let declarations = publisher.declarations(sources.snapshot(), &cancel).unwrap();
    let views: Vec<_> = providers
        .iter()
        .map(|module| {
            declarations
                .render_native_document(module, &cancel)
                .unwrap()
        })
        .collect();
    let capture = providers
        .iter()
        .zip(&views)
        .map(|(module, view)| CapturedSource {
            key: view.uri.clone(),
            display_name: view.uri.clone(),
            module: module.records().identity.clone(),
            text: view.text.clone(),
        })
        .collect();
    sources.replace_generated(capture, &cancel).unwrap();
    let mut editor = AnalysisDatabase::default();
    editor.set_import_environment(declarations.import_environment().clone());
    (editor, views)
}

pub(crate) fn test_analysis() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(standard_modules());
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
