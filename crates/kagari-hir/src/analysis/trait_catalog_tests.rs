use super::*;
use crate::{declarations::DeclarationId, typeck::CallTarget, types::BuiltinType};
use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

fn insert(sources: &mut SourceDatabase, name: &str, text: &str) -> FileId {
    sources
        .bind_module(
            name,
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec![name.into()],
            },
        )
        .unwrap();
    sources.set(name, text.into(), SourceLayer::Base).unwrap()
}
fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

#[test]
fn method_catalog_preserves_checked_bounds_beside_an_invalid_constraint() {
    use crate::typeck::ConstraintTarget;
    let mut sources = SourceDatabase::default();
    let root = insert(
        &mut sources,
        "root",
        "trait Reader<T: HashKey> { fn read<U>(self, value: U) -> U where U: Missing + HashKey; }",
    );
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    let file = snapshot.file(root).unwrap();
    assert_eq!(file.result().diagnostics().len(), 1);
    assert_eq!(
        file.result().diagnostics()[0].kind.code(),
        "KG_TYPE_UNKNOWN_TRAIT"
    );
    let facts = file.result().facts();
    let method = &facts.aggregates.traits().next().unwrap().methods[0];
    let signature = facts
        .typed
        .functions
        .iter()
        .find(|f| f.name == "read")
        .unwrap();
    assert_eq!(method.bounds, signature.bounds);
    assert_eq!(method.bounds.len(), 2);
    for parameter in &method.generic_params {
        assert!(matches!(
            method.bounds[&crate::types::TypeId::Generic(parameter.clone())].as_slice(),
            [ConstraintTarget::Standard(_)]
        ));
    }
    assert_ne!(
        method.generic_params[0].owner,
        method.generic_params[1].owner
    );
    assert!(file.result().clone().into_codegen().is_err());
}

#[test]
fn imported_methods_keep_checked_parameters_self_types_and_source_targets() {
    let mut sources = SourceDatabase::default();
    let left = insert(
        &mut sources,
        "left",
        "pub trait View { fn read(self, value: i32) -> i32; fn copy(self) -> Self; } fn broken() { missing; }",
    );
    let right = insert(
        &mut sources,
        "right",
        "pub trait View { fn read(self, value: String) -> String; }",
    );
    insert(
        &mut sources,
        "unused",
        "pub trait Hidden { fn read(self); }",
    );
    insert(&mut sources, "facade", "pub use pkg::left::View;");
    let text = "use pkg::facade::View as L; use pkg::right::View as R; fn left(x: L) -> i32 { x.read(7) } fn right(x: R) -> String { x.read(\"ok\") } fn copy(x: L) -> L { x.copy() } fn bad(x: L) -> i32 { x.read(true) }";
    let root = insert(&mut sources, "root", text);
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    let analysis = snapshot.file(root).unwrap();
    let facts = analysis.result().facts();
    assert_eq!(facts.aggregates.traits().count(), 2);
    assert!(
        facts
            .aggregates
            .traits()
            .all(|t| t.id.module.path != ["unused"])
    );
    assert_eq!(
        analysis.type_at(text.find("x.read(7)").unwrap() + 2),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        analysis.type_at(text.find("x.read(\"ok\")").unwrap() + 2),
        Some(TypeId::Builtin(BuiltinType::String))
    );
    assert_eq!(
        analysis
            .definition_at(text.find("x.read(7)").unwrap() + 2)
            .unwrap()
            .location
            .file,
        left
    );
    assert_eq!(
        analysis
            .definition_at(text.find("x.read(\"ok\")").unwrap() + 2)
            .unwrap()
            .location
            .file,
        right
    );
    assert_eq!(
        analysis.type_at(text.find("x.copy()").unwrap() + 2),
        analysis.type_at(text.find("copy(x: L").unwrap() + "copy(x: ".len())
    );
    assert_eq!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .filter(|d| d.kind.code() == "KG_TYPE_ARGUMENT_TYPE_MISMATCH")
            .count(),
        1
    );
    assert!(
        !analysis
            .result()
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_RESOLVE_UNKNOWN_NAME")
    );
    let methods = facts
        .lowered
        .module
        .body
        .expressions()
        .filter_map(
            |(id, _)| match facts.typed.type_table.call_resolution(id)?.target {
                CallTarget::TraitMethod { method, .. } => Some(method),
                _ => None,
            },
        )
        .collect::<Vec<_>>();
    assert_eq!(methods.len(), 4);
    assert_ne!(methods[0], methods[1]);
    assert_eq!(methods[0], methods[3]);
}

#[test]
fn invalid_method_parameter_does_not_discard_later_parameters_or_cascade_errors() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "lib",
        "pub trait View { fn read(self, bad: Missing, later: i32) -> i32; }",
    );
    let text = "use pkg::lib::View; fn bad(x: View) -> i32 { x.read(true, \"wrong\") } fn good(x: i32) -> i32 { x }";
    let root = insert(&mut sources, "root", text);
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    let file = snapshot.file(root).unwrap();
    let method = &file
        .result()
        .facts()
        .aggregates
        .traits()
        .next()
        .unwrap()
        .methods[0];
    assert_eq!(method.params.len(), 3);
    assert_eq!(method.params[1].ty, TypeId::Error);
    assert_eq!(method.params[2].ty, TypeId::Builtin(BuiltinType::I32));
    assert_eq!(
        file.result()
            .diagnostics()
            .iter()
            .filter(|d| d.kind.code() == "KG_TYPE_ARGUMENT_TYPE_MISMATCH")
            .count(),
        1
    );
    assert!(
        file.definition_at(text.find("x.read").unwrap() + 2)
            .is_some()
    );
    assert_eq!(
        file.type_at(text.rfind(" x }").unwrap() + 1),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn ambiguous_methods_have_no_call_target_and_duplicate_bounds_do_not_create_ambiguity() {
    for bounds in ["Left + Right", "Right + Left"] {
        let text = format!(
            "trait Left {{ fn get(self, n: i32) -> i32; }} trait Right {{ fn get(self, n: i32) -> i32; }} fn read<T: {bounds}>(x: T) -> i32 {{ x.get(missing) }}"
        );
        let mut sources = SourceDatabase::default();
        let file = insert(&mut sources, "root", &text);
        let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
        let file = snapshot.file(file).unwrap();
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_AMBIGUOUS_METHOD"),
            "{:?}",
            file.result().diagnostics()
        );
        assert_eq!(
            file.result()
                .diagnostics()
                .iter()
                .filter(|d| d.kind.code() == "KG_RESOLVE_UNKNOWN_NAME")
                .count(),
            1
        );
        let facts = file.result().facts();
        assert!(
            !facts.lowered.module.body.expressions().any(|(id, _)| facts
                .typed
                .type_table
                .call_resolution(id)
                .is_some())
        );
        assert!(
            file.definition_at(text.find("x.get").unwrap() + 2)
                .is_none()
        );
        assert!(file.result().clone().into_codegen().is_err());
    }
    let mut sources = SourceDatabase::default();
    let file = insert(
        &mut sources,
        "root",
        "trait Left { fn get(self) -> i32; } fn read<T: Left + Left>(x: T) -> i32 { x.get() }",
    );
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    assert!(
        snapshot
            .file(file)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
}

#[test]
fn duplicate_trait_and_impl_methods_are_diagnosed_before_codegen() {
    for text in [
        "trait View { fn get(self) -> i32; fn get(self) -> bool; } fn call(x: View) { x.get(); }",
        "struct Point {} impl Point { fn get(self) -> i32 { 1 } fn get(self) -> i32 { 2 } }",
    ] {
        let mut sources = SourceDatabase::default();
        let file = insert(&mut sources, "root", text);
        let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
        let file = snapshot.file(file).unwrap();
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_RESOLVE_DUPLICATE_METHOD")
        );
        assert!(file.result().clone().into_codegen().is_err());
    }
}

#[test]
fn imported_method_contract_edits_invalidate_consumers_and_preserve_old_snapshots() {
    let mut sources = SourceDatabase::default();
    let library = "pub trait View { fn read(self) -> i32; } fn helper() -> i32 { 1 }";
    insert(&mut sources, "lib", library);
    let text = "use pkg::lib::View; fn read(x: View) -> i32 { x.read() } fn helper() {}";
    let root = insert(&mut sources, "root", text);
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let before = old.file(root).unwrap().result().facts();
    let DeclarationId::Definition(owner) = &before
        .declarations
        .iter()
        .find(|d| d.name == "read")
        .unwrap()
        .id
    else {
        panic!("function");
    };
    let body = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(body.diagnostics().is_empty());
    sources
        .set(
            "root",
            text.replace("fn helper() {}", "fn helper() { val n = 1; }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let reused = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(reused.reused_bodies(), 1);
    sources
        .set(
            "lib",
            library.replace("{ 1 }", "{ val x = 2; x }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let refreshed = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(refreshed.diagnostics().is_empty());
    let after_body_edit = analyze(&mut db, &sources);
    assert!(
        before.aggregates.same_contracts(
            &after_body_edit
                .file(root)
                .unwrap()
                .result()
                .facts()
                .aggregates
        )
    );
    sources
        .set(
            "lib",
            library.replace("fn read(self) -> i32", "fn read(self) -> String"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let invalidated = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(invalidated.reused_bodies(), 0);
    assert!(
        invalidated
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_TYPE_RETURN_TYPE_MISMATCH")
    );
    let fresh = AnalysisDatabase::default()
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    invalidated.type_table().assert_same_source_facts(
        fresh.type_table(),
        invalidated.lowered().module.body.arena(),
        fresh.lowered().module.body.arena(),
    );
    assert_eq!(
        old.file(root)
            .unwrap()
            .type_at(text.find("x.read").unwrap() + 2),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}
