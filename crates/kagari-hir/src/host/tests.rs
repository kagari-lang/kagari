use super::*;
use crate::{LanguageFeatureProfile, analysis::AnalysisDatabase};
use kagari_common::{
    host_interface::{HostParameter, HostPassingStyle},
    source_database::{SourceDatabase, SourceLayer},
};

#[test]
fn offline_type_queries_preserve_member_contracts_and_reject_stale_ids() {
    use kagari_common::host_interface::{HostFieldDeclaration, HostTypeDeclaration};
    let mut declaration = HostTypeDeclaration::new("model.Player");
    declaration.fields.push(HostFieldDeclaration::new(
        &declaration.id,
        "score",
        HostValueType::I32,
    ));
    declaration.fields[0].documentation = "Current score".into();
    let interface = HostInterface {
        paths: vec![],
        types: vec![declaration.clone()],
        functions: vec![],
    };
    let old =
        HostDeclarations::new(HostInterface::from_bytes(&interface.to_bytes().unwrap()).unwrap())
            .unwrap();
    let id = old.resolve_type("model::Player").unwrap();
    assert_eq!(old.nominal_type(&declaration.id), Some(id));
    assert_eq!(
        old.type_declaration(id).unwrap().fields[0].documentation,
        "Current score"
    );
    assert!(old.module("model").is_some());
    declaration.fields[0].ty = HostValueType::I64;
    let new = HostDeclarations::new(HostInterface {
        paths: vec![],
        types: vec![declaration],
        functions: vec![],
    })
    .unwrap();
    assert!(new.type_declaration(id).is_none());
    assert_eq!(
        new.type_declaration(new.resolve_type("model::Player").unwrap())
            .unwrap()
            .fields[0]
            .ty,
        HostValueType::I64
    );
    assert_eq!(
        old.type_declaration(id).unwrap().fields[0].ty,
        HostValueType::I32
    );
}

pub(super) fn declaration() -> HostFunctionDeclaration {
    let mut declaration = HostFunctionDeclaration::new(
        "demo.echo",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    declaration.documentation = "Echo an integer".into();
    declaration
}

#[test]
fn snapshots_own_host_declarations_and_invalidate_body_reuse_on_input_change() {
    let mut sources = SourceDatabase::default();
    let text = "use demo::{echo as echo}; use demo as api; fn good() -> i32 { echo(api::echo(demo::echo(7))) }";
    let file = sources
        .set("host.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = AnalysisDatabase::default();
    let original = HostDeclarations::new(HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![declaration()],
    })
    .unwrap();
    let old_id = original.resolve("demo::echo").unwrap();
    database.set_host_declarations(original.clone());
    let profile = LanguageFeatureProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let first = database
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let first_file = first.file(file).unwrap();
    assert!(
        first_file.result().diagnostics().is_empty(),
        "{:?}",
        first_file.result().diagnostics()
    );
    assert_eq!(
        first_file
            .host_function_at(text.rfind("demo::echo").unwrap() + "demo::".len())
            .unwrap()
            .id,
        declaration().id
    );
    let cached = database
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    assert!(Arc::ptr_eq(first_file, cached.file(file).unwrap()));
    let mut changed = declaration();
    changed.return_type = HostValueType::String;
    changed.documentation = "Now returns text".into();
    let updated = HostDeclarations::new(HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![changed],
    })
    .unwrap();
    assert!(updated.function(old_id).is_none());
    database.set_host_declarations(updated);
    let second = database
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let second_file = second.file(file).unwrap();
    assert_eq!(first.revision(), second.revision());
    assert_ne!(first.host_revision(), second.host_revision());
    assert_eq!(second_file.result().facts().typed.reused_bodies, 0);
    assert!(!second_file.result().diagnostics().is_empty());
    let offset = text.rfind("demo::echo").unwrap() + "demo::".len();
    assert_eq!(
        first_file.host_function_at(offset).unwrap().return_type,
        HostValueType::I32
    );
    assert_eq!(
        second_file.host_function_at(offset).unwrap().return_type,
        HostValueType::String
    );
    assert_eq!(
        first_file.host_function_at(offset).unwrap().documentation,
        "Echo an integer"
    );
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
        let mut database = AnalysisDatabase::default();
        database.set_host_declarations(
            HostDeclarations::new(HostInterface {
                paths: vec![],
                types: Vec::new(),
                functions: vec![declaration()],
            })
            .unwrap(),
        );
        let snapshot = database
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile {
                    allow_host_calls: true,
                    ..Default::default()
                },
                &Default::default(),
            )
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

#[test]
fn host_catalog_rejects_ambiguous_or_unspellable_paths() {
    for name in ["std.echo", "demo::echo", "demo.2bad"] {
        assert!(
            HostDeclarations::new(HostInterface {
                paths: vec![],
                types: Vec::new(),
                functions: vec![HostFunctionDeclaration::new(
                    name,
                    vec![],
                    HostValueType::Unit
                )]
            })
            .is_err()
        );
    }
    assert!(
        HostDeclarations::new(HostInterface {
            paths: vec![],
            types: Vec::new(),
            functions: vec![
                declaration(),
                HostFunctionDeclaration::new("demo.echo.child", vec![], HostValueType::Unit)
            ]
        })
        .is_err()
    );
}
