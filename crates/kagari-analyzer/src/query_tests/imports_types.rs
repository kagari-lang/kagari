use crate::query_tests::imports_tests::{analyze, insert};
use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_hir::{
    imports::{
        NamespaceId, ResolvedTarget,
        catalog::{LookupContext, NamespaceResult},
    },
    types::TypeId,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;
use std::sync::*;

#[test]
fn imported_annotations_preserve_nominal_identity_and_definition_locations() {
    let mut db = SourceDatabase::default();
    let types = insert(
        &mut db,
        "types",
        "pub struct Data { val value: i32 } pub enum Choice { First } pub trait View { fn read(self) -> i32; }",
    );
    let text = "use pkg::types as lib; use pkg::types::Data as D; use pkg::types::Choice; use pkg::types::View; struct Holder { val items: [D] } fn pass(x: D) -> lib::Data { val y: lib::Data = x; y } fn choice(x: Choice) -> Choice { x } fn view(x: View) -> View { x }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    assert!(
        snapshot
            .file(types)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let signature = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    assert_eq!(signature.params[0].ty, signature.return_type);
    assert!(
        matches!(&signature.return_type, TypeId::Struct(id) if file.definitions().resolve(id.declaration).unwrap().to_path().module.path == ["types"])
    );
    for needle in [
        "[D]",
        "D)",
        "lib::Data {",
        "lib::Data =",
        "Choice)",
        "View)",
    ] {
        let offset = text.find(needle).unwrap()
            + match needle {
                "[D]" => 1,
                "lib::Data {" | "lib::Data =" => "lib::".len(),
                _ => 0,
            };
        let declaration = snapshot.queries().definition_at(root, offset).unwrap();
        assert_eq!(declaration.source_location().unwrap().file, types);
        assert!(
            db.snapshot()
                .contains(declaration.source_location().unwrap())
        );
    }
}

#[test]
fn exported_signatures_use_imported_types_before_callers_are_checked() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "models", "pub struct Data { val value: i32 }");
    let api = insert(
        &mut db,
        "api",
        "use pkg::models::Data; pub fn pass(x: Data) -> Data { x }",
    );
    let text = "use pkg::api::pass; use pkg::models::Data; fn forward(x: Data) -> Data { pass(x) }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    for (_, node) in snapshot.module_graph().modules() {
        let diagnostics = snapshot.file(node.file).unwrap().result().diagnostics();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
    let file = snapshot.file(root).unwrap();
    let imported = file
        .queries()
        .source_function_at(text.find("pass(x)").unwrap())
        .unwrap();
    assert_eq!(imported.id.unit.file, api);
    let local = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "forward")
        .unwrap();
    assert_eq!(local.return_type, imported.signature.return_type);
    assert_eq!(local.params[0].ty, imported.signature.params[0].ty);
}

#[test]
fn shared_facade_resolution_rejects_stale_targets_and_terminates_cycles() {
    let mut db = SourceDatabase::default();
    let types = insert(&mut db, "types", "pub struct Data { val value: i32 }");
    let text = "use pkg::types::Data;";
    let root = insert(&mut db, "root", text);
    let offset = text.find("types::").unwrap();
    let mut analysis = test_analysis();
    let cancel = Default::default();
    let first = analysis.snapshot(db.snapshot(), &cancel).unwrap();
    let target = first
        .queries()
        .source_target_at(root, offset)
        .unwrap()
        .target;
    let ResolvedTarget::Namespace(namespace @ NamespaceId::Module(unit)) = target else {
        panic!("expected the physical types module, got {target:?}");
    };
    assert_eq!(unit.file, types);
    let importer = first
        .module_graph()
        .catalog
        .definitions()
        .lookup_module(first.file(root).unwrap().source().module_identity())
        .unwrap();
    let ctx = LookupContext::new(&importer);
    let first_catalog = &first.module_graph().catalog;
    assert_eq!(
        first_catalog.namespace_of(&ctx, &target, &cancel).unwrap(),
        NamespaceResult::Found(namespace)
    );
    db.set(
        "mem://types",
        "pub struct Data { val value: bool }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let second = analysis.snapshot(db.snapshot(), &cancel).unwrap();
    let second_catalog = &second.module_graph().catalog;
    let current = second
        .queries()
        .source_target_at(root, offset)
        .unwrap()
        .target;
    let ResolvedTarget::Namespace(current_namespace @ NamespaceId::Module(current_unit)) = current
    else {
        panic!("expected the updated types module, got {current:?}");
    };
    assert_eq!(current_unit.module, unit.module);
    assert_ne!(current_unit.revision, unit.revision);
    assert_eq!(
        second_catalog
            .namespace_of(&ctx, &current, &cancel)
            .unwrap(),
        NamespaceResult::Found(current_namespace)
    );
    assert_eq!(
        second_catalog.namespace_of(&ctx, &target, &cancel).unwrap(),
        NamespaceResult::StaleSource
    );
    assert_eq!(
        first_catalog.namespace_of(&ctx, &target, &cancel).unwrap(),
        NamespaceResult::Found(namespace)
    );

    let text = "pub use pkg::b::Alias;";
    let a = insert(&mut db, "a", text);
    let b = insert(&mut db, "b", "pub use pkg::a::Alias;");
    let cycle = analysis.snapshot(db.snapshot(), &cancel).unwrap();
    let queries = cycle.queries();
    let prefix = queries
        .source_target_at(a, text.find("b::").unwrap())
        .unwrap();
    assert!(
        matches!(prefix.target, ResolvedTarget::Namespace(NamespaceId::Module(unit)) if unit.file == b)
    );
    assert!(
        queries
            .source_targets_at(a, text.find("Alias").unwrap())
            .is_empty()
    );
    assert!(
        cycle
            .file(a)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| {
                matches!(&d.kind, DiagnosticKind::UnknownName { name } if name == "pkg::b::Alias")
            })
    );

    cancel.cancel();
    assert!(first_catalog.namespace_of(&ctx, &target, &cancel).is_err());
}

#[test]
fn type_facades_resolve_before_signatures_including_module_aliases() {
    let mut db = SourceDatabase::default();
    let types = insert(&mut db, "types", "pub struct Data { val value: i32 }");
    insert(
        &mut db,
        "facade",
        "pub use pkg::types::Data as Renamed; pub use pkg::types as models;",
    );
    let text = "use pkg::facade::Renamed as D; use pkg::facade::models as m; fn pass(x: D) -> m::Data { x }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let declaration = snapshot
        .queries()
        .definition_at(root, text.find("m::Data").unwrap() + "m::".len())
        .unwrap();
    assert_eq!(declaration.source_location().unwrap().file, types);
    assert_eq!(declaration.name, "Data");
    let signature = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    assert_eq!(signature.params[0].ty, signature.return_type);
}

#[test]
fn transitive_type_visibility_changes_invalidate_signatures_and_old_targets_remain() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "types", "pub struct Data { val value: i32 }");
    let facade = insert(&mut db, "facade", "pub use pkg::types::Data;");
    let text = "use pkg::facade::Data; fn pass(x: Data) -> Data { x } fn good() -> i32 { 42 }";
    let root = insert(&mut db, "root", text);
    let mut analysis = test_analysis();
    let first = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    let location = text.find("Data)").unwrap();
    let old = first
        .queries()
        .definition_at(root, location)
        .unwrap()
        .clone();
    db.set(
        "mem://types",
        "struct Data { val value: i32 }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let second = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(
        first.file(facade).unwrap().source().revision(),
        second.file(facade).unwrap().source().revision()
    );
    assert!(!Arc::ptr_eq(
        first.file(root).unwrap().signatures(),
        second.file(root).unwrap().signatures()
    ));
    assert!(second.queries().definition_at(root, location).is_none());
    assert_eq!(first.queries().definition_at(root, location), Some(&old));
    let file = second.file(root).unwrap();
    assert!(
        file.result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownType { .. }))
    );
    assert_eq!(
        file.queries().type_at(text.find("42").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn generic_parameters_shadow_type_imports_and_missing_annotations_keep_other_facts() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "types", "pub struct Data { val value: i32 }");
    let text = "use pkg::types::Data as T; fn keep<T>(x: T) -> T { x } fn number() -> i32 { keep(42) } fn broken(x: Absent, y: T) -> T { y }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let file = snapshot.file(root).unwrap();
    assert_eq!(
        file.result().diagnostics().len(),
        1,
        "{:?}",
        file.result().diagnostics()
    );
    assert_eq!(
        file.queries().type_at(text.find("keep(42)").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let broken = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "broken")
        .unwrap();
    assert_eq!(broken.params[0].ty, TypeId::Error);
    assert!(matches!(broken.params[1].ty, TypeId::Struct(_)));
    assert_eq!(broken.params[1].ty, broken.return_type);
}
