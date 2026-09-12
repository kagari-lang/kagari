use kagari_common::{
    SourceFile,
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle, HostValueType,
    },
};
use kagari_embed::{
    ArtifactOptions, CompileOptions, ExecutionContext, HostExposurePolicy, KagariEngine,
    LoadOptions,
};
use kagari_runtime::{CapabilitySet, LanguageProfile, host::HostFunction, value::Value};
use std::sync::{Arc, Mutex};

fn declaration() -> HostFunctionDeclaration {
    HostFunctionDeclaration::new(
        "demo.echo",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    )
}

#[test]
fn offline_declarations_compile_without_a_runtime_then_link_and_execute() {
    let definition = declaration();
    let encoded = HostInterface {
        functions: vec![definition.clone()],
    }
    .to_bytes()
    .unwrap();
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface::from_bytes(&encoded).unwrap())
        .unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let artifact = engine.compile_to_artifact(SourceFile::new("host.kgr", "use demo::echo as call; use demo as api; fn main() -> i32 { call(api::echo(demo::echo(39))) }"), CompileOptions { language_profile: profile }, ArtifactOptions::default()).unwrap();
    assert_eq!(
        artifact.module.host_interface.functions,
        vec![definition.clone()]
    );
    let artifact =
        kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext {
        language_profile: profile,
        capabilities: CapabilitySet {
            host_calls: true,
            ..Default::default()
        },
        host_policy: HostExposurePolicy {
            allowed_host_functions: vec!["demo.echo".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let called = calls.clone();
    let mut runtime = engine.runtime(context.clone());
    assert!(
        runtime
            .load_module(artifact.clone(), LoadOptions::default())
            .is_err()
    );
    runtime
        .register_host_function(HostFunction::new(definition, move |args| {
            called.lock().unwrap().push(args.to_vec());
            let [Value::I32(value)] = args else {
                unreachable!()
            };
            Ok(Value::I32(value + 1))
        }))
        .unwrap();
    let loaded = runtime
        .load_module(artifact, LoadOptions::default())
        .unwrap();
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            vec![Value::I32(39)],
            vec![Value::I32(40)],
            vec![Value::I32(41)]
        ]
    );
}

#[test]
fn host_calls_require_profile_and_cannot_run_in_scalar_constants() {
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface {
            functions: vec![declaration()],
        })
        .unwrap();
    let disabled = engine
        .compile_source(
            SourceFile::new("disabled.kgr", "fn main() -> i32 { demo::echo(7) }"),
            CompileOptions::default(),
        )
        .unwrap_err();
    assert!(format!("{disabled:?}").contains("KG_PROFILE_FEATURE_DISABLED"));
    let constant = engine.compile_source(
        SourceFile::new(
            "const.kgr",
            "const N: i32 = demo::echo(7); fn main() -> i32 { N }",
        ),
        CompileOptions {
            language_profile: LanguageProfile {
                allow_host_calls: true,
                ..Default::default()
            },
        },
    );
    assert!(constant.is_err());
}
