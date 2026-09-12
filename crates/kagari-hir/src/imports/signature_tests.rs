use super::tests::{analyze, insert};
use crate::{
    analysis::AnalysisDatabase,
    typeck::CallTarget,
    types::{BuiltinType, TypeId},
};
use kagari_common::{
    DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use std::sync::Arc;

#[test]
fn signatures_and_imported_calls_survive_dependency_body_errors() {
    let mut db = SourceDatabase::default();
    let library = insert(
        &mut db,
        "library",
        "pub fn echo(x: i32) -> i32 { missing } fn broken(x: Absent) -> Absent { unknown } const BAD: i32 = 1 / 0;",
    );
    let text = "use pkg::library as lib; fn good() -> i32 { lib::echo(42) } fn bad() { lib::echo(true); lib::echo(); }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let file = snapshot.file(root).unwrap();
    let signature = file
        .source_function_at(text.find("lib::echo(42)").unwrap())
        .unwrap();
    assert_eq!(signature.id.file, library);
    assert_eq!(
        signature.signature.return_type,
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        file.type_at(text.find("lib::echo(42)").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        file.result()
            .diagnostics()
            .iter()
            .map(|d| d.kind.code())
            .collect::<Vec<_>>(),
        [
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
            "KG_TYPE_CALL_ARITY_MISMATCH"
        ]
    );
    assert!(
        file.result()
            .facts()
            .lowered
            .module
            .body
            .expressions()
            .any(|(id, _)| {
                file.result()
                    .facts()
                    .typed
                    .type_table
                    .call_resolution(id)
                    .is_some_and(|call| call.target == CallTarget::SourceFunction(signature.id))
            })
    );
    let library = snapshot.file(library).unwrap();
    assert!(
        library
            .signatures()
            .diagnostics()
            .iter()
            .all(|d| matches!(d.kind, DiagnosticKind::UnknownType { .. }))
    );
    assert!(library.result().diagnostics().len() > library.signatures().diagnostics().len());
    let broken = library
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "broken")
        .unwrap();
    assert_eq!(broken.params[0].ty, TypeId::Error);
    assert_eq!(broken.return_type, TypeId::Error);
}

#[test]
fn imported_nominal_signatures_distinguish_same_named_types() {
    let mut db = SourceDatabase::default();
    for module in ["left", "right"] {
        insert(
            &mut db,
            module,
            "pub struct Value { number: i32 } pub fn make() -> Value { Value { number: 1 } } pub fn accept(value: Value) -> i32 { value.number }",
        );
    }
    let text = "use pkg::left as l; use pkg::right as r; fn good() -> i32 { l::accept(l::make()) } fn bad() -> i32 { l::accept(r::make()) }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let file = snapshot.file(root).unwrap();
    assert_eq!(
        file.result().diagnostics().len(),
        1,
        "{:?}",
        file.result().diagnostics()
    );
    assert!(matches!(
        file.result().diagnostics()[0].kind,
        DiagnosticKind::ArgumentTypeMismatch { .. }
    ));
    let left = file
        .source_function_at(text.find("l::make()").unwrap())
        .unwrap();
    let right = file
        .source_function_at(text.find("r::make()").unwrap())
        .unwrap();
    assert_ne!(left.signature.return_type, right.signature.return_type);
    assert_ne!(left.declaration, right.declaration);
}

#[test]
fn facade_signature_changes_invalidate_unchanged_transitive_callers() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "library", "pub fn value() -> i32 { 1 }");
    let facade = insert(&mut db, "facade", "pub use pkg::library::value;");
    let text = "use pkg::facade::value; fn main() -> i32 { val x = value(); x }";
    let root = insert(&mut db, "root", text);
    let unrelated = insert(&mut db, "unrelated", "fn other() -> i32 { 9 }");
    let mut analysis = AnalysisDatabase::default();
    let first = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(first.file(root).unwrap().result().diagnostics().is_empty());
    let old_binding = first
        .file(root)
        .unwrap()
        .definition_at(text.rfind("x }").unwrap())
        .unwrap()
        .clone();
    db.set(
        "mem://library",
        "pub fn value() -> bool { true }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let second = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert_eq!(
        first.file(facade).unwrap().source().revision(),
        second.file(facade).unwrap().source().revision()
    );
    assert_eq!(
        first.file(root).unwrap().source().revision(),
        second.file(root).unwrap().source().revision()
    );
    assert!(!Arc::ptr_eq(
        first.file(root).unwrap(),
        second.file(root).unwrap()
    ));
    assert!(second.declaration(&old_binding.id).is_none());
    assert_eq!(first.declaration(&old_binding.id), Some(&old_binding));
    assert!(Arc::ptr_eq(
        first.file(root).unwrap().signatures(),
        second.file(root).unwrap().signatures()
    ));
    assert!(Arc::ptr_eq(
        first.file(unrelated).unwrap(),
        second.file(unrelated).unwrap()
    ));
    assert_eq!(
        second
            .file(root)
            .unwrap()
            .result()
            .facts()
            .typed
            .reused_bodies,
        0
    );
    assert!(
        second
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::ReturnTypeMismatch { .. }))
    );
    let offset = text.rfind("value()").unwrap();
    assert_eq!(
        first
            .file(root)
            .unwrap()
            .source_function_at(offset)
            .unwrap()
            .signature
            .return_type,
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        second
            .file(root)
            .unwrap()
            .source_function_at(offset)
            .unwrap()
            .signature
            .return_type,
        TypeId::Builtin(BuiltinType::Bool)
    );
}

#[test]
fn cyclic_modules_keep_signatures_for_tooling_but_cannot_generate_code() {
    let mut db = SourceDatabase::default();
    let text = "use pkg::b; pub fn a() -> i32 { b::b() }";
    let a = insert(&mut db, "a", text);
    insert(&mut db, "b", "use pkg::a; pub fn b() -> i32 { a::a() }");
    let snapshot = analyze(&db);
    let file = snapshot.file(a).unwrap();
    assert_eq!(
        file.type_at(text.find("b::b()").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert!(
        file.result()
            .diagnostics()
            .iter()
            .all(|d| matches!(d.kind, DiagnosticKind::CyclicImport { .. }))
    );
    assert!(file.result().clone().into_codegen().is_err());
}
