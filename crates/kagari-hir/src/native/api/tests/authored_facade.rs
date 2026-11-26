use crate::tests::standard::standard_modules;
use crate::{
    analysis::{AnalysisDatabase, core_source::packaged_modules},
    declarations::DeclarationId,
    program::exports::TypeExport,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::namespaces;
use std::slice;

#[test]
fn std_facade_exports_the_authored_owner_on_both_input_routes() {
    let identity = namespaces::module("std", "cmp");
    let facade = standard_modules()
        .into_iter()
        .find(|m| m.records().identity == identity)
        .unwrap();
    let providers = slice::from_ref(&facade);
    let cmp = packaged_modules()
        .iter()
        .find(|m| m.module_identity() == &namespaces::trait_owner("PartialEq"))
        .unwrap();
    let cancel = Default::default();
    let mut db = AnalysisDatabase::new("/facade").unwrap();
    db.set_native_modules(providers.to_vec());
    // A real facade, but only the facade is registered. Its registered target
    // resolves through the supplied source package, not a fabricated cmp provider.
    let sources = SourceDatabase::default();
    let published = db.declarations(sources.snapshot(), &cancel).unwrap();
    let view = published.render_native_document(&facade, &cancel).unwrap();
    assert!(
        view.text
            .contains("pub use core::cmp::PartialEq as PartialEq;")
    );
    let direct = db.snapshot(sources.snapshot(), &cancel).unwrap();
    let mut documents = sources;
    documents.bind_module(&view.uri, identity.clone()).unwrap();
    documents
        .set(&view.uri, view.text, SourceLayer::Base)
        .unwrap();
    let mut editor = AnalysisDatabase::default();
    editor.set_import_environment(published.import_environment().clone());
    let parsed = editor.snapshot(documents.snapshot(), &cancel).unwrap();
    for snapshot in [direct, parsed] {
        let graph = snapshot.module_graph();
        let handle = graph.catalog.definitions.lookup_module(&identity).unwrap();
        let checked = snapshot
            .check_program(graph.node(&handle).unwrap().file, &cancel)
            .unwrap();
        let TypeExport::Declaration(target) =
            checked.exports().namespaces()[&handle].types["PartialEq"]
        else {
            panic!("expected canonical trait export")
        };
        let loc = snapshot
            .declaration(&DeclarationId::Definition(target))
            .unwrap()
            .source_location()
            .unwrap();
        assert_eq!(loc.file, cmp.id());
        assert_eq!(&cmp.text()[loc.range.start..loc.range.end], "PartialEq");
    }
}
