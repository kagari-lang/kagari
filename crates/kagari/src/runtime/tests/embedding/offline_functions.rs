use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::{KagariEngine, source::ArtifactOptions},
    program::PreparedProgram,
    runtime::LoadOptions,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::{ModuleIdentity, PackageId, map::DefinitionContext};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::{
    host::HostCallContext,
    native::{
        builder::ModuleBuilder, declarations::FunctionDecl, module::NativeModule, types::Type,
    },
    value::Value,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::declaration::host::{HostCallPolicy, HostPassingStyle};
use std::sync::{Arc, Mutex};

fn echo_module(result: Type, calls: Arc<Mutex<Vec<i32>>>) -> NativeModule {
    let returns_bool = result.abi() == Type::bool().abi();
    let mut builder =
        ModuleBuilder::new("demo", &StandardDeclarations::default().catalog().unwrap()).unwrap();
    let echo = builder
        .define_function(
            FunctionDecl::new("echo")
                .parameter("value", Type::i32())
                .returns(result)
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::Owned],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    builder
        .bind(echo, move |_: &HostCallContext<'_>, args: &[Value]| {
            let [Value::I32(value)] = args else {
                unreachable!()
            };
            calls.lock().unwrap().push(*value);
            Ok(if returns_bool {
                Value::Bool(true)
            } else {
                Value::I32(value + 1)
            })
        })
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn offline_host_facades_preserve_linking_and_backend_call_traces() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let module = echo_module(Type::i32(), calls.clone());
    let engine =
        KagariEngine::with_native_modules(Default::default(), vec![module.clone()]).unwrap();
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
        .emit_artifact(&checked, ArtifactOptions::default())
        .unwrap();
    let comparison = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&comparison, &Default::default())
        .unwrap();
    let declaration = module
        .to_declaration()
        .import_into(&comparison, &Default::default())
        .unwrap();
    let expected = declaration
        .records()
        .native_declarations(declaration.definitions())
        .unwrap();
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
        if module.identity.path == ["root"] {
            assert_eq!(module.native_imports.len(), 1);
            assert_eq!(
                module.native_imports[0].instance.declaration,
                expected[0].declaration
            );
        } else if module.identity.package.0 == "pkg" {
            assert!(module.native_imports.is_empty());
        } else if module.identity.package.0 == "demo" {
            assert_eq!(module.native_declarations, expected);
        }
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
        calls.lock().unwrap().clear();
        let wrong_calls = Arc::new(Mutex::new(Vec::new()));
        let incompatible_engine = KagariEngine::with_native_modules(
            Default::default(),
            vec![echo_module(Type::bool(), wrong_calls.clone())],
        )
        .unwrap();
        let mut incompatible = incompatible_engine.runtime(context.clone());
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
        assert!(wrong_calls.lock().unwrap().is_empty());
        let mut missing = KagariEngine::default().runtime(context.clone());
        assert!(
            missing
                .load_program(
                    &PreparedProgram::from_artifact(
                        artifact.clone(),
                        &Default::default(),
                        &Default::default()
                    )
                    .unwrap(),
                    LoadOptions::default(),
                )
                .is_err()
        );
        let mut runtime = engine.runtime(context.clone());
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
                        ProgramEntry::Main,
                        backend,
                        &context.cancellation,
                    )
                    .unwrap();
                runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
            } else {
                runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
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
    let calls = Arc::new(Mutex::new(Vec::new()));
    let module = echo_module(Type::i32(), calls.clone());
    let engine =
        KagariEngine::with_native_modules(Default::default(), vec![module.clone()]).unwrap();
    let artifact = engine.compile_to_artifact(SourceFile::new("host.kgr", "use demo::echo as call; use demo as api; fn main() -> i32 { call(api::echo(api::echo(39))) }"),  ArtifactOptions::default()).unwrap();
    let comparison = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&comparison, &Default::default())
        .unwrap();
    let declaration = module
        .to_declaration()
        .import_into(&comparison, &Default::default())
        .unwrap();
    let expected = declaration
        .records()
        .native_declarations(declaration.definitions())
        .unwrap();
    let imports =
        &artifact.records().program.modules[artifact.records().program.root.index()].native_imports;
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].instance.declaration, expected[0].declaration);
    assert!(calls.lock().unwrap().is_empty());
    let artifact =
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext {
        ..Default::default()
    };
    let mut missing = KagariEngine::default().runtime(context.clone());
    assert!(
        missing
            .load_program(
                &PreparedProgram::from_artifact(
                    artifact.clone(),
                    &Default::default(),
                    &Default::default()
                )
                .unwrap(),
                LoadOptions::default(),
            )
            .is_err()
    );
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, LoadOptions::default())
        .unwrap();
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(*calls.lock().unwrap(), [39, 40, 41]);
}

#[test]
fn declared_host_calls_compile_but_cannot_run_in_scalar_constants() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        vec![echo_module(Type::i32(), calls.clone())],
    )
    .unwrap();
    assert!(
        engine
            .compile_source(SourceFile::new(
                "host.kgr",
                "use demo; fn main() -> i32 { demo::echo(7) }"
            ))
            .is_ok()
    );
    let constant = engine.compile_source(SourceFile::new(
        "const.kgr",
        "use demo; const N: i32 = demo::echo(7); fn main() -> i32 { N }",
    ));
    assert!(constant.is_err());
    assert!(calls.lock().unwrap().is_empty());
}
