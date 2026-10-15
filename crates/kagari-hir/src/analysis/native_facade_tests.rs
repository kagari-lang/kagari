//! Native facade queries retain common declaration identity and immutable snapshots.
use crate::{
    analysis::AnalysisDatabase,
    tests::{native::function_module, test_analysis},
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, metadata::DefinitionMetadata,
};
use kagari_source::{
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::{declaration::module::ModuleDecl, scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

fn install_echo(
    db: &mut AnalysisDatabase,
    result: BuiltinType,
) -> Arc<DefinitionMetadata<ModuleDecl>> {
    let module = function_module(
        ModuleIdentity {
            package: PackageId("demo".into()),
            path: vec![],
        },
        "echo",
        vec![("value", Ty::Builtin(BuiltinType::I32))],
        Ty::Builtin(result),
    );
    let (definitions, mut records) = module.as_ref().clone().into_parts();
    let echo = records
        .definition(DefinitionKind::Function, "echo", &definitions)
        .unwrap();
    records.documentation.insert(
        echo,
        if result == BuiltinType::I32 {
            "Echo an integer".into()
        } else {
            "Now returns text".into()
        },
    );
    let module =
        Arc::new(DefinitionMetadata::checked(definitions, records, &Default::default()).unwrap());
    let mut modules = foundation_catalog::shared().unwrap();
    modules.push(module.clone());
    db.set_native_modules(modules);
    module
}

fn insert(sources: &mut SourceDatabase, name: &str, text: &str) -> FileId {
    let path = format!("mem://{name}");
    sources
        .bind_module(
            &path,
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec![name.into()],
            },
        )
        .unwrap();
    sources.set(&path, text.into(), SourceLayer::Base).unwrap()
}

#[test]
fn facade_bindings_keep_offline_host_identity_queries_and_revision_invalidation() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "facade",
        "pub use demo::echo as call; pub use demo as service;",
    );
    insert(
        &mut sources,
        "relay",
        "pub use pkg::facade::call; pub use pkg::facade::service;",
    );
    let text = "use pkg::relay::call as invoke; use pkg::relay::service as api; use pkg::relay; fn main() -> i32 { invoke(api::echo(relay::call(relay::service::echo(1)))) }";
    let root = insert(&mut sources, "root", text);
    let mut db = test_analysis();
    let declaration = install_echo(&mut db, BuiltinType::I32);
    let echo = declaration
        .records()
        .definition(DefinitionKind::Function, "echo", declaration.definitions())
        .unwrap();
    let echo = declaration.definitions().resolve(echo).unwrap().to_path();

    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = old.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    old.check_program(root, &Default::default()).unwrap();
    for spelling in [
        "pkg::relay::call",
        "invoke(api",
        "api::echo",
        "relay::call(relay",
        "relay::service::echo",
    ] {
        assert_eq!(
            file.source_function_at(
                text.find(spelling).unwrap() + spelling.rfind("::").map_or(0, |colon| colon + 2),
            )
            .unwrap()
            .declaration,
            file.definitions().lookup(&echo).unwrap()
        );
    }
    let again = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(Arc::ptr_eq(file, again.file(root).unwrap()));
    install_echo(&mut db, BuiltinType::String);
    let current = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(old.revision(), current.revision());
    let current_snapshot = current;
    let current = current_snapshot.file(root).unwrap();
    assert_eq!(current.result().facts().typed.reused_bodies, 0);
    assert!(!current.result().diagnostics().is_empty());
    let offset = text.find("pkg::relay::call").unwrap() + "pkg::relay::".len();
    assert_eq!(
        file.source_function_at(offset)
            .unwrap()
            .signature
            .return_type,
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        current
            .source_function_at(offset)
            .unwrap()
            .signature
            .return_type,
        TypeId::Builtin(BuiltinType::String)
    );
    let call_offset = text.find("api::echo").unwrap() + "api::".len();
    assert_eq!(
        file.call_signature_at(call_offset).unwrap().result,
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        current.call_signature_at(call_offset).unwrap().result,
        TypeId::Builtin(BuiltinType::String)
    );
    assert_eq!(
        old.documentation_at(root, call_offset)
            .unwrap()
            .documentation,
        "Echo an integer"
    );
    assert_eq!(
        current_snapshot
            .documentation_at(root, call_offset)
            .unwrap()
            .documentation,
        "Now returns text"
    );
}

#[test]
fn host_facades_do_not_override_local_shadowing_or_duplicate_export_errors() {
    for facade in [
        "pub use demo::echo as invoke;",
        "pub use demo::echo as invoke; pub use demo::echo as invoke;",
    ] {
        let mut sources = SourceDatabase::default();
        insert(&mut sources, "facade", facade);
        let text = "use pkg::facade::invoke; fn bad() { val invoke = 1; invoke(2); } fn good() -> i32 { 7 }";
        let root = insert(&mut sources, "root", text);
        let mut db = test_analysis();
        install_echo(&mut db, BuiltinType::I32);
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(root).unwrap();
        assert!(
            file.source_function_at(text.find("invoke(2)").unwrap())
                .is_none()
        );
        assert_eq!(
            file.type_at(text.rfind('7').unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(snapshot.check_program(root, &Default::default()).is_err());
    }
}

#[test]
fn invalid_imports_and_calls_keep_neighbor_facts_but_block_codegen() {
    let cases = [
        (
            "use missing::echo; fn good() -> i32 { 42 }",
            "KG_RESOLVE_UNKNOWN_NAME",
        ),
        (
            "use demo::echo as same; use demo::echo as same; fn good() -> i32 { 42 }",
            "KG_RESOLVE_DUPLICATE_IMPORT",
        ),
        (
            "use demo::echo as good; fn good() -> i32 { 42 }",
            "KG_RESOLVE_DUPLICATE_IMPORT",
        ),
        (
            "fn bad() { demo::echo(true); } fn good() -> i32 { 42 }",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
        ),
    ];
    for (text, code) in cases {
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("bad.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut database = test_analysis();
        install_echo(&mut database, BuiltinType::I32);
        let snapshot = database
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(file).unwrap();
        assert!(
            analysis
                .result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == code),
            "{text}: {:?}",
            analysis.result().diagnostics()
        );
        assert_eq!(
            analysis.type_at(text.rfind("42").unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(analysis.result().clone().into_codegen().is_err());
    }
}
