use super::*;
use crate::{LanguageFeatureProfile, analysis::AnalysisDatabase};
use kagari_common::{
    identity::{FileId, ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

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
    let text = "use pkg::relay::call as invoke; use pkg::relay::service as api; use pkg::relay as relay; fn main() -> i32 { invoke(api::echo(relay::call(relay::service::echo(1)))) }";
    let root = insert(&mut sources, "root", text);
    let mut db = AnalysisDatabase::default();
    let declaration = super::tests::declaration();
    db.set_host_declarations(
        HostDeclarations::new(HostInterface {
            field_paths: vec![],
            types: Vec::new(),
            functions: vec![declaration.clone()],
        })
        .unwrap(),
    );
    let profile = LanguageFeatureProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let old = db
        .snapshot(sources.snapshot(), profile, &Default::default())
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
            file.host_function_at(
                text.find(spelling).unwrap() + spelling.rfind("::").map_or(0, |colon| colon + 2),
            )
            .unwrap()
            .id,
            declaration.id
        );
    }
    let again = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    assert!(Arc::ptr_eq(file, again.file(root).unwrap()));
    let mut changed = declaration;
    changed.return_type = HostValueType::String;
    db.set_host_declarations(
        HostDeclarations::new(HostInterface {
            field_paths: vec![],
            types: Vec::new(),
            functions: vec![changed],
        })
        .unwrap(),
    );
    let current = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let current = current.file(root).unwrap();
    assert_eq!(current.result().facts().typed.reused_bodies, 0);
    assert!(!current.result().diagnostics().is_empty());
    let offset = text.find("pkg::relay::call").unwrap() + "pkg::relay::".len();
    assert_eq!(
        file.host_function_at(offset).unwrap().return_type,
        HostValueType::I32
    );
    assert_eq!(
        current.host_function_at(offset).unwrap().return_type,
        HostValueType::String
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
        let mut db = AnalysisDatabase::default();
        db.set_host_declarations(
            HostDeclarations::new(HostInterface {
                field_paths: vec![],
                types: Vec::new(),
                functions: vec![super::tests::declaration()],
            })
            .unwrap(),
        );
        let snapshot = db
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile {
                    allow_host_calls: true,
                    ..Default::default()
                },
                &Default::default(),
            )
            .unwrap();
        let file = snapshot.file(root).unwrap();
        assert!(
            file.host_function_at(text.find("invoke(2)").unwrap())
                .is_none()
        );
        assert_eq!(
            file.type_at(text.rfind('7').unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(snapshot.check_program(root, &Default::default()).is_err());
    }
}
