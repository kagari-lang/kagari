use super::*;
use crate::types::{BuiltinType, TypeId};
use kagari_common::source_database::{SourceDatabase, SourceLayer};

#[test]
fn aggregate_bounds_are_checked_for_annotations_constructors_and_forwarded_parameters() {
    for text in [
        "struct Key<T: HashKey> { val value: T } fn bad(x: Key<f32>) {}",
        "struct Key<T: HashKey> { val value: T } fn bad() { Key { value: 1.5 }; }",
        "enum Key<T: HashKey> { Value(T) } fn bad() { Key::Value(1.5); }",
        "struct Key<T: HashKey> { val value: T } fn bad<T>(value: T) { Key { value: value }; }",
    ] {
        let source = SourceFile::new("bounds.kgr", text);
        let analysis = crate::analyze_source(&source, Default::default());
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_STANDARD_CONSTRAINT_NOT_SATISFIED"),
            "{text}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err());
    }
    let source = SourceFile::new(
        "bounds.kgr",
        "struct Key<T: HashKey> { val value: T } enum Items<T: HashKey> { Values(Set<T>) } fn pass<T: HashKey>(value: T) -> Key<T> { Key { value: value } }",
    );
    let analysis = crate::analyze_source(&source, Default::default());
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
}

#[test]
fn generic_templates_construct_and_project_distinct_instances() {
    let text = "struct Cell<T> { var value: T } enum Packet<T> { Data(T) } fn get<T>(cell: Cell<T>) -> T { cell.value } fn main() -> (i32, String, Packet<i32>) { val a = Cell { value: 7 }; val b = Cell { value: \"hello\" }; a.value = 8; (get(a), get(b), Packet::Data(a.value)) }";
    let source = SourceFile::new("generic.kgr", text);
    let result = crate::analyze_source(&source, Default::default());
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    let structure = facts.aggregates.structures().next().unwrap();
    let enumeration = facts.aggregates.enumerations().next().unwrap();
    assert_eq!(structure.generic_params.len(), 1);
    assert_eq!(enumeration.generic_params.len(), 1);
    assert_ne!(structure.generic_params[0], enumeration.generic_params[0]);
    assert_eq!(
        structure.fields[0].ty,
        TypeId::Generic(structure.generic_params[0].clone())
    );
    let instances = facts
        .lowered
        .module
        .body
        .expressions()
        .filter(|(_, expr)| matches!(expr.kind, crate::hir::ExprKind::StructInit { .. }))
        .map(|(id, _)| facts.typed.type_table.expr_type(id).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(instances.len(), 2);
    assert_ne!(instances[0], instances[1]);
    for (instance, expected) in instances
        .iter()
        .zip([BuiltinType::I32, BuiltinType::String])
    {
        let TypeId::Struct(instance) = instance else {
            panic!("struct instance");
        };
        assert_eq!(instance.arguments, [TypeId::Builtin(expected)]);
        assert_eq!(instance.declaration, structure.id);
    }
}

#[test]
fn generic_type_parameters_navigate_and_rebase_without_losing_their_owner() {
    let text = "fn before() {} struct Cell<T> { val value: T } enum Packet<T> { Data(T) } fn read(x: Cell<i32>) -> i32 { x.value }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("generic.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let old_file = old.file(id).unwrap();
    assert!(
        old_file.result().diagnostics().is_empty(),
        "{:?}",
        old_file.result().diagnostics()
    );
    let parameter = old_file
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    assert_eq!(
        parameter.location.range.start,
        text.find("Cell<T").unwrap() + 5
    );
    let payload = old_file
        .definition_at(text.find("Data(T").unwrap() + 5)
        .unwrap();
    assert_ne!(payload.id, parameter.id);
    let edit = text.replace("fn before() {}", "fn before() { val n = 1; }");
    sources
        .set("generic.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let changed = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let file = changed.file(id).unwrap();
    assert!(file.signatures_reused());
    let new_parameter = file
        .definition_at(edit.find("value: T").unwrap() + 7)
        .unwrap();
    assert_eq!(new_parameter.id, parameter.id);
    assert_eq!(
        new_parameter.location.range.start,
        edit.find("Cell<T").unwrap() + 5
    );
    assert!(matches!(
        file.type_at(edit.rfind("value").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    ));
}
