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
fn offline_host_facades_preserve_linking_and_backend_call_traces() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    let definition = declaration();
    engine
        .set_host_interface(
            HostInterface::from_bytes(
                &HostInterface {
                    paths: vec![],
                    types: Vec::new(),
                    functions: vec![definition.clone()],
                }
                .to_bytes()
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let mut root = None;
    for (name, source) in [
        ("facade", "pub use demo::echo as call; pub use demo as api;"),
        (
            "relay",
            "pub use pkg::facade::call; pub use pkg::facade::api;",
        ),
        (
            "root",
            "use pkg::relay::call; use pkg::relay::api; use pkg::relay as facade; fn main() -> i32 { call(api::echo(facade::call(facade::api::echo(1)))) }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        root = Some(
            engine
                .set_source(&path, source.into(), SourceLayer::Base)
                .unwrap(),
        );
    }
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_jit: true,
        ..Default::default()
    };
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root.unwrap(),
            CompileOptions {
                language_profile: profile,
            },
            &Default::default(),
        )
        .unwrap();
    let artifact = engine
        .emit_bytecode(&checked, ArtifactOptions::default())
        .unwrap();
    assert_eq!(
        artifact
            .program
            .modules
            .iter()
            .map(|m| m.identity.path[0].as_str())
            .collect::<Vec<_>>(),
        ["facade", "relay", "root"]
    );
    for module in &artifact.program.modules {
        assert_eq!(
            module.host_interface.functions,
            if module.identity.path == ["root"] {
                vec![definition.clone()]
            } else {
                vec![]
            }
        );
    }
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            language_profile: profile,
            capabilities: CapabilitySet {
                host_calls: true,
                jit: true,
                ..Default::default()
            },
            host_policy: HostExposurePolicy {
                allowed_host_functions: vec!["demo.echo".into()],
                ..Default::default()
            },
            jit_policy: if jit {
                kagari_embed::JitPolicy::Enabled
            } else {
                kagari_embed::JitPolicy::Disabled
            },
            ..Default::default()
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let called = calls.clone();
        let mut incompatible = engine.runtime(context.clone());
        let mut wrong = definition.clone();
        wrong.return_type = HostValueType::Bool;
        incompatible
            .register_host_function(HostFunction::new(wrong, |_, _| {
                panic!("linking cannot invoke the callback")
            }))
            .unwrap();
        assert!(
            incompatible
                .load_program(artifact.clone(), LoadOptions::default())
                .is_err()
        );
        let mut runtime = engine.runtime(context.clone());
        assert!(
            runtime
                .load_program(artifact.clone(), LoadOptions::default())
                .is_err()
        );
        runtime
            .register_host_function(HostFunction::new(definition.clone(), move |_, args| {
                let [Value::I32(value)] = args else {
                    unreachable!()
                };
                called.lock().unwrap().push(*value);
                Ok(Value::I32(value + 1))
            }))
            .unwrap();
        let loaded = runtime
            .load_program(artifact, LoadOptions::default())
            .unwrap();
        assert!(calls.lock().unwrap().is_empty());
        let mut denied = context.clone();
        denied.jit_policy = kagari_embed::JitPolicy::Disabled;
        denied.host_policy.allowed_host_functions = vec!["call".into()];
        assert!(runtime.execute(&loaded, "main", &[], &denied).is_err());
        assert!(calls.lock().unwrap().is_empty());
        let mut backend = jit.then(|| kagari_jit_cranelift::CraneliftBackend::for_host().unwrap());
        for _ in 0..2 {
            let report = if let Some(backend) = &mut backend {
                runtime.execute_with_backend(&loaded, "main", &[], &context, backend)
            } else {
                runtime.execute(&loaded, "main", &[], &context)
            }
            .unwrap();
            assert_eq!(report.return_value, Value::I32(5));
        }
        assert_eq!(*calls.lock().unwrap(), [1, 2, 3, 4, 1, 2, 3, 4]);
    }
}

#[test]
fn offline_declarations_compile_without_a_runtime_then_link_and_execute() {
    let definition = declaration();
    let encoded = HostInterface {
        paths: vec![],
        types: Vec::new(),
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
        artifact.program.modules[artifact.program.root.index()]
            .host_interface
            .functions,
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
            .load_program(artifact.clone(), LoadOptions::default())
            .is_err()
    );
    runtime
        .register_host_function(HostFunction::new(definition, move |_, args| {
            called.lock().unwrap().push(args.to_vec());
            let [Value::I32(value)] = args else {
                unreachable!()
            };
            Ok(Value::I32(value + 1))
        }))
        .unwrap();
    let loaded = runtime
        .load_program(artifact, LoadOptions::default())
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
            paths: vec![],
            types: Vec::new(),
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
