use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::{
    ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_embed::{
    context::{ExecutionContext, JitPolicy},
    engine::{KagariEngine, source::ArtifactOptions},
    program::PreparedProgram,
    runtime::LoadOptions,
};
use kagari_runtime::{host::HostFunction, value::Value};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_types::host_interface::{
    HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle,
    value_type::HostValueType,
};
use std::sync::{Arc, Mutex};

fn declaration() -> DefinitionMetadata<HostFunctionDeclaration> {
    let context = DefinitionContext::new().unwrap();
    let declaration = HostFunctionDeclaration::new(
        &context,
        "demo.echo",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    )
    .unwrap();
    DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default()).unwrap()
}

#[test]
fn offline_host_facades_preserve_linking_and_backend_call_traces() {
    let engine = KagariEngine::default();
    let definition = declaration();
    let interface = interface(&definition);
    engine
        .set_host_interface(
            HostInterface::from_bytes(
                &interface
                    .records()
                    .to_bytes(interface.definitions())
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

    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default())
        .unwrap();
    let artifact = engine
        .emit_bytecode(&checked, ArtifactOptions::default())
        .unwrap();
    let comparison = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&comparison, &Default::default())
        .unwrap();
    let expected = definition
        .clone()
        .import_into(&comparison, &Default::default())
        .unwrap()
        .into_records();
    assert_eq!(
        artifact
            .records()
            .program
            .modules
            .iter()
            .filter(|module| module.identity.package.0 == "pkg")
            .map(|m| m.identity.path[0].as_str())
            .collect::<Vec<_>>(),
        ["facade", "relay", "root"]
    );
    for module in &artifact.records().program.modules {
        assert_eq!(
            module.host_interface.functions,
            if module.identity.path == ["root"] {
                vec![expected.clone()]
            } else {
                vec![]
            }
        );
    }
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let called = calls.clone();
        let mut incompatible = engine.runtime(context.clone());
        let (definitions, mut wrong) = definition.clone().into_parts();
        wrong.return_type = HostValueType::Bool;
        incompatible
            .register_host_function(HostFunction::new(
                DefinitionMetadata::checked(definitions, wrong, &Default::default()).unwrap(),
                |_, _| panic!("linking cannot invoke the callback"),
            ))
            .unwrap();
        assert!(
            incompatible
                .load_program(
                    &PreparedProgram::from_artifact(
                        artifact.clone(),
                        &Default::default(),
                        &Default::default()
                    )
                    .unwrap(),
                    LoadOptions::default()
                )
                .is_err()
        );
        let mut runtime = engine.runtime(context.clone());
        assert!(
            runtime
                .load_program(
                    &PreparedProgram::from_artifact(
                        artifact.clone(),
                        &Default::default(),
                        &Default::default()
                    )
                    .unwrap(),
                    LoadOptions::default()
                )
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
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, LoadOptions::default())
            .unwrap();
        assert!(calls.lock().unwrap().is_empty());
        let mut backend =
            jit.then(|| kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap());
        for _ in 0..2 {
            let report = if let Some(backend) = &mut backend {
                let prepared = runtime
                    .prepare_native(
                        &loaded_program,
                        &loaded,
                        "main",
                        backend,
                        &context.cancellation,
                    )
                    .unwrap();
                runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
            } else {
                runtime.execute(&loaded, "main", &[], &context)
            }
            .unwrap();
            assert_eq!(
                report
                    .return_value
                    .value(runtime.runtime().gc())
                    .expect("retained execution result"),
                Value::I32(5)
            );
        }
        assert_eq!(*calls.lock().unwrap(), [1, 2, 3, 4, 1, 2, 3, 4]);
    }
}

#[test]
fn offline_declarations_compile_without_a_runtime_then_link_and_execute() {
    let definition = declaration();
    let interface = interface(&definition);
    let encoded = interface
        .records()
        .to_bytes(interface.definitions())
        .unwrap();
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface::from_bytes(&encoded).unwrap())
        .unwrap();

    let artifact = engine.compile_to_artifact(SourceFile::new("host.kgr", "use demo::echo as call; use demo as api; fn main() -> i32 { call(api::echo(demo::echo(39))) }"),  ArtifactOptions::default()).unwrap();
    let comparison = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&comparison, &Default::default())
        .unwrap();
    let expected = definition
        .clone()
        .import_into(&comparison, &Default::default())
        .unwrap()
        .into_records();
    assert_eq!(
        artifact.records().program.modules[artifact.records().program.root.index()]
            .host_interface
            .functions,
        vec![expected.clone()]
    );
    let artifact =
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext {
        ..Default::default()
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let called = calls.clone();
    let mut runtime = engine.runtime(context.clone());
    assert!(
        runtime
            .load_program(
                &PreparedProgram::from_artifact(
                    artifact.clone(),
                    &Default::default(),
                    &Default::default()
                )
                .unwrap(),
                LoadOptions::default()
            )
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
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, LoadOptions::default())
        .unwrap();
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
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
fn declared_host_calls_compile_but_cannot_run_in_scalar_constants() {
    let engine = KagariEngine::default();
    engine
        .set_host_interface(interface(&declaration()))
        .unwrap();
    assert!(
        engine
            .compile_source(SourceFile::new(
                "host.kgr",
                "fn main() -> i32 { demo::echo(7) }"
            ))
            .is_ok()
    );
    let constant = engine.compile_source(SourceFile::new(
        "const.kgr",
        "const N: i32 = demo::echo(7); fn main() -> i32 { N }",
    ));
    assert!(constant.is_err());
}

fn interface(
    definition: &DefinitionMetadata<HostFunctionDeclaration>,
) -> DefinitionMetadata<HostInterface> {
    DefinitionMetadata::checked(
        definition.definitions().clone(),
        HostInterface {
            paths: vec![],
            types: vec![],
            functions: vec![definition.records().clone()],
        },
        &Default::default(),
    )
    .unwrap()
}
