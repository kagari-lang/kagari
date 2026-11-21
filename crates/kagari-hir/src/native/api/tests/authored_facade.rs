use crate::tests::standard::standard_modules;
use crate::{
    analysis::{AnalysisDatabase, core_source::packaged_modules, error::AnalysisError},
    declarations::DeclarationId,
    native::render::declaration_source_with_external,
    program::exports::TypeExport,
};
use kagari_source::source_database::SourceSnapshot;
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
        .find(|m| m.source.module_identity() == &namespaces::trait_owner("PartialEq"))
        .unwrap();
    let cancel = Default::default();
    let mut db = AnalysisDatabase::new("/facade").unwrap();
    db.set_native_modules(providers.to_vec());
    // A real facade, but only the facade is registered. Its registered target
    // resolves through the supplied source package, not a fabricated cmp provider.
    let view = declaration_source_with_external(&facade, providers, db.inputs().library_modules())
        .unwrap();
    assert!(
        view.text
            .contains("pub use core::cmp::PartialEq as PartialEq;")
    );
    for supplied in [false, true] {
        if supplied {
            db.set_native_sources(providers.to_vec(), vec![view.clone()])
                .unwrap();
        }
        let snapshot = db.snapshot(SourceSnapshot::default(), &cancel).unwrap();
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
        assert_eq!(loc.file, cmp.source.id());
        assert_eq!(
            &cmp.source.text()[loc.range.start..loc.range.end],
            "PartialEq"
        );
    }

    // External aliases are inputs, not fixed canonical-package rewrites. A
    // retained materialization using the old spelling must be revalidated.
    let mut renamed_owner = cmp.clone();
    renamed_owner.package_alias = Some("ordering".into());
    db.replace_core_fixture(renamed_owner);
    assert!(
        matches!(db.snapshot(SourceSnapshot::default(), &cancel), Err(AnalysisError::NativeApi(e)) if e.0.contains("differs from authoritative registration"))
    );
    let renamed =
        declaration_source_with_external(&facade, providers, db.inputs().library_modules())
            .unwrap();
    assert!(
        renamed
            .text
            .contains("pub use ordering::cmp::PartialEq as PartialEq;")
    );
    db.set_native_sources(providers.to_vec(), vec![renamed])
        .unwrap();
    let snapshot = db.snapshot(SourceSnapshot::default(), &cancel).unwrap();
    let graph = snapshot.module_graph();
    let handle = graph.catalog.definitions.lookup_module(&identity).unwrap();
    snapshot
        .check_program(graph.node(&handle).unwrap().file, &cancel)
        .unwrap();
}
