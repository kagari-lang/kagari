use crate::analysis::*;
use crate::tests::standard::standard_modules;
use crate::{
    aggregates::traits::MethodDefault,
    declarations::DeclarationId,
    semantic::signatures::FunctionImplementation,
    tests::{native_fixture as fixture, test_analysis},
};
use kagari_common::identity::{ModuleIdentity, PackageId, metadata::DefinitionMetadata};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::ty::NominalTy;

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
    if !db
        .native_modules
        .iter()
        .any(|module| module.records().identity == fixture::module().records().identity)
    {
        db.set_native_modules(
            standard_modules()
                .into_iter()
                .chain([fixture::module()])
                .collect(),
        );
    }
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn native_default_bodies_are_checked_without_synthesizing_impl_bodies() {
    let mut sources = SourceDatabase::default();
    let root = insert(
        &mut sources,
        "root",
        "use demo::bindings::NativeRead; struct Values {} impl NativeRead for Values {}",
    );
    let mut db = test_analysis();
    let snapshot = analyze(&mut db, &sources);
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let authoring_catalog = file.to_unverified(&Default::default()).unwrap();
    let definitions = authoring_catalog.definitions();
    let catalog = &authoring_catalog.records().facts().aggregates;
    let implementation = catalog
        .implementations()
        .find(|item| {
            definitions.resolve(item.id).unwrap().module()
                == file.result().facts().lowered.source.module_identity()
        })
        .unwrap();
    assert!(implementation.methods.is_empty());
    assert_eq!(
        catalog
            .implementation_methods(definitions, implementation)
            .len(),
        2
    );
    assert!(
        file.result()
            .facts()
            .typed
            .signatures
            .functions()
            .is_empty()
    );
    let contract = catalog
        .trait_(&implementation.trait_type.declaration)
        .unwrap();
    assert_eq!(contract.methods.len(), 2);
    for method in &contract.methods {
        assert!(matches!(method.default, Some(MethodDefault::Script)));
    }
    assert!(contract.methods[0].declaration.source_location().is_none());
    let owner = definitions
        .resolve(contract.methods[0].id)
        .unwrap()
        .module()
        .clone();
    let native = snapshot
        .files()
        .find(|candidate| candidate.source().module_identity() == &owner)
        .unwrap();
    let native_file = native.source().id();
    let defaults = || {
        native
            .result()
            .facts()
            .typed
            .signatures
            .functions()
            .iter()
            .filter(|function| matches!(function.implementation, FunctionImplementation::Script))
    };
    assert!(defaults().all(|function| {
        native
            .result()
            .facts()
            .typed
            .type_table
            .call_resolution(
                native
                    .result()
                    .facts()
                    .lowered
                    .module
                    .block(
                        native.result().facts().lowered.module.functions[function.id.index()]
                            .implementation
                            .script_body()
                            .unwrap(),
                    )
                    .tail_expr
                    .unwrap(),
            )
            .is_some()
    }));
    let retained = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(Arc::ptr_eq(native, retained.file(native_file).unwrap()));

    // Hash is a valid rendered trait requirement, but NativeRead's default body
    // cannot prove it for Self. Signature queries must not check that body.
    let (definitions, mut invalid) = fixture::module().as_ref().clone().into_parts();
    let additional_bound = invalid
        .functions
        .iter()
        .find(|function| function.name == "choose")
        .unwrap()
        .bounds[0]
        .constraints[0]
        .clone();
    let helper = invalid
        .functions
        .iter_mut()
        .find(|function| function.name == "default_read")
        .unwrap();
    helper.bounds[0].constraints.push(additional_bound);
    helper.bounds[0].constraints = NominalTy::normalize_bounds(
        helper.bounds[0].constraints.clone(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    db.set_native_modules(
        standard_modules()
            .into_iter()
            .chain([Arc::new(
                DefinitionMetadata::checked(definitions, invalid, &Default::default()).unwrap(),
            )])
            .collect(),
    );
    let signatures = db
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let native_file = signatures
        .declaration_snapshot()
        .files()
        .find(|file| file.source().name() == native.source().name())
        .unwrap()
        .source()
        .id();
    let signature_file = signatures.file(native_file).unwrap();
    assert!(
        signature_file.diagnostics().is_empty(),
        "{:?}",
        signature_file.diagnostics()
    );
    assert!(defaults().all(|function| {
        signature_file
            .signatures()
            .facts()
            .type_table()
            .call_resolution(
                native
                    .result()
                    .facts()
                    .lowered
                    .module
                    .block(
                        native.result().facts().lowered.module.functions[function.id.index()]
                            .implementation
                            .script_body()
                            .unwrap(),
                    )
                    .tail_expr
                    .unwrap(),
            )
            .is_none()
    }));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(db.snapshot(sources.snapshot(), &cancel).is_err());
    let failed = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let failed = failed.file(native_file).unwrap();
    assert!(
        failed
            .result()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(
                &diagnostic.kind,
                DiagnosticKind::GenericBoundNotSatisfied { trait_name, .. } if trait_name == "Hash"
            )),
        "{:?}",
        failed.result().diagnostics()
    );
    for (name, valid) in [("read", false), ("fixed", true)] {
        let function = failed
            .result()
            .facts()
            .typed
            .signatures
            .functions()
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        assert_eq!(
            failed
                .result()
                .facts()
                .typed
                .valid_bodies
                .contains(&function.id),
            valid,
            "{name}"
        );
    }
    assert!(defaults().all(|function| {
        native
            .result()
            .facts()
            .typed
            .type_table
            .call_resolution(
                native
                    .result()
                    .facts()
                    .lowered
                    .module
                    .block(
                        native.result().facts().lowered.module.functions[function.id.index()]
                            .implementation
                            .script_body()
                            .unwrap(),
                    )
                    .tail_expr
                    .unwrap(),
            )
            .is_some()
    }));
}

#[test]
fn installed_non_overridable_default_rejects_a_script_replacement() {
    let mut sources = SourceDatabase::default();
    let root = insert(
        &mut sources,
        "root",
        "use demo::bindings::NativeRead; struct Values {} impl NativeRead for Values { fn fixed(self) -> i32 { 7 } }",
    );
    let snapshot = analyze(&mut test_analysis(), &sources);
    let file = snapshot.file(root).unwrap();
    assert!(file.result().diagnostics().iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::TraitMethodMismatch { method_name, reason, .. }
            if method_name == "fixed" && reason == "this method declaration forbids overriding"
    )));
    assert!(file.result().clone().into_codegen().is_err());
}

#[test]
fn changing_a_requirement_to_a_script_default_invalidates_contract_reuse() {
    let mut sources = SourceDatabase::default();
    let text = "trait Action { fn act(self) -> i32; } struct Point {} impl Action for Point {}";
    let root = insert(&mut sources, "root", text);
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    sources
        .set(
            "root",
            text.replace("-> i32;", "-> i32 { 1 }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let new = analyze(&mut db, &sources);
    let before = old.file(root).unwrap();
    let after = new.file(root).unwrap();
    assert!(!before.result().diagnostics().is_empty());
    assert!(
        after.result().diagnostics().is_empty(),
        "{:?}",
        after.result().diagnostics()
    );
    let authoring_before = before.to_unverified(&Default::default()).unwrap();
    let before = &authoring_before.records().facts().aggregates;
    let authoring_after = after.to_unverified(&Default::default()).unwrap();
    let after = &authoring_after.records().facts().aggregates;
    assert!(!before.same_contracts(after));
    let a = before
        .traits()
        .find(|item| item.declaration.name == "Action")
        .unwrap();
    let b = after
        .traits()
        .find(|item| item.declaration.name == "Action")
        .unwrap();
    assert_eq!(a.methods[0].id, b.methods[0].id);
    assert_eq!(a.methods[0].default, None);
    assert_eq!(b.methods[0].default, Some(MethodDefault::Script));
}

#[test]
fn method_catalog_preserves_checked_bounds_beside_an_invalid_constraint() {
    let mut sources = SourceDatabase::default();
    let root = insert(
        &mut sources,
        "root",
        "use std::hash::Hash; trait Reader<T: Eq + Hash> { fn read<U>(self, value: U) -> U where U: Missing + Eq + Hash; }",
    );
    let snapshot = analyze(&mut test_analysis(), &sources);
    let file = snapshot.file(root).unwrap();
    assert_eq!(file.result().diagnostics().len(), 1);
    assert_eq!(
        file.result().diagnostics()[0].kind.code(),
        "KG_TYPE_UNKNOWN_TRAIT"
    );
    let authoring_facts = file.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let method = &facts
        .aggregates
        .traits()
        .find(|t| t.declaration.name == "Reader")
        .unwrap()
        .methods[0];
    let signature = facts
        .typed
        .signatures
        .functions()
        .iter()
        .find(|f| f.name == "read")
        .unwrap();
    assert_eq!(method.bounds, signature.bounds);
    assert_eq!(method.bounds.len(), 2);
    for parameter in &method.generic_params {
        assert!(matches!(
            method.bounds[&crate::types::TypeId::Generic(parameter.clone())].as_slice(),
            [_, _]
        ));
    }
    assert_ne!(
        method.generic_params[0].owner,
        method.generic_params[1].owner
    );
    assert!(file.result().clone().into_codegen().is_err());
}

#[test]
fn duplicate_trait_and_impl_methods_are_diagnosed_before_codegen() {
    for text in [
        "trait View { fn get(self) -> i32; fn get(self) -> bool; } fn call(x: View) { x.get(); }",
        "struct Point {} impl Point { fn get(self) -> i32 { 1 } fn get(self) -> i32 { 2 } }",
    ] {
        let mut sources = SourceDatabase::default();
        let file = insert(&mut sources, "root", text);
        let snapshot = analyze(&mut test_analysis(), &sources);
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
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let authoring_before = old
        .file(root)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let before = authoring_before.records().facts();
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
                .to_unverified(&Default::default())
                .unwrap()
                .records()
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
    let mut fresh_db = test_analysis();
    let fresh_declarations = fresh_db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let query = invalidated.definitions().resolve(*owner).unwrap().to_path();
    let fresh_owner = fresh_declarations
        .file(root)
        .unwrap()
        .declarations()
        .definitions()
        .lookup(&query)
        .unwrap();
    let fresh = fresh_db
        .body(sources.snapshot(), &fresh_owner, &Default::default())
        .unwrap()
        .unwrap();
    invalidated.type_table().assert_same_source_facts(
        invalidated.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        invalidated.lowered().module.nodes.arena(),
        fresh.lowered().module.nodes.arena(),
    );
}
