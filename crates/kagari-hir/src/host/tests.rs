use super::*;
use crate::{
    analysis::AnalysisDatabase, callable::CallableSignature, native::NativeBinding,
    typeck::FunctionImplementation,
};
use kagari_common::{
    host_interface::{HostParameter, HostPassingStyle},
    source_database::{SourceDatabase, SourceLayer},
};

#[test]
fn offline_type_queries_preserve_member_contracts_and_reject_stale_ids() {
    use kagari_common::host_interface::type_declaration::{
        HostFieldDeclaration, HostTypeDeclaration,
    };
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
fn checked_host_callables_retain_provider_contracts_and_reject_other_inputs() {
    let mut declaration = declaration();
    declaration.params[0].ty = HostValueType::String;
    declaration.params[0].passing = HostPassingStyle::SharedBorrow;

    declaration.effects.may_call_host_services = true;
    declaration.effects.may_trap = true;
    let interface = HostInterface {
        functions: vec![declaration.clone()],
        ..Default::default()
    };
    let original = HostDeclarations::new(interface.clone()).unwrap();
    let other = HostDeclarations::new(interface).unwrap();
    let id = original.resolve("demo::echo").unwrap();
    let callable = original.callable(id).unwrap();
    assert_eq!(callable.contract(), &declaration);
    assert_eq!(callable.name(), "demo.echo");
    assert_eq!(
        callable.implementation(),
        FunctionImplementation::Native(NativeBinding::Host(id))
    );
    assert_eq!(
        callable.parameters().collect::<Vec<_>>(),
        [("value", &TypeId::Builtin(BuiltinType::String))]
    );
    assert_eq!(callable.return_type(), &TypeId::Builtin(BuiltinType::I32));
    assert!(callable.generic_params().is_empty());
    assert!(callable.bounds().is_none());
    assert!(other.callable(id).is_none());
    let other_id = other.resolve("demo::echo").unwrap();
    assert_ne!(id, other_id);
    assert_eq!(other.callable(other_id).unwrap().contract(), &declaration);
    assert_eq!(original.callable(id).unwrap().contract(), &declaration);
}

#[test]
fn snapshots_own_host_declarations_and_invalidate_body_reuse_on_input_change() {
    let mut sources = SourceDatabase::default();
    let text =
        "use demo::{echo}; use demo as api; fn good() -> i32 { echo(api::echo(demo::echo(7))) }";
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

    let first = database
        .snapshot(sources.snapshot(), &Default::default())
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
        .snapshot(sources.snapshot(), &Default::default())
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
        .snapshot(sources.snapshot(), &Default::default())
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
    assert_eq!(
        first_file.call_signature_at(offset).unwrap().result,
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        second_file.call_signature_at(offset).unwrap().result,
        TypeId::Builtin(BuiltinType::String)
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
