use kagari_common::{
    cancellation::CancellationToken,
    identity::{FileId, ModuleIdentity, PackageId},
    source_database::SourceLayer,
};
use kagari_embed::{
    BytecodeArtifact, CompileOptions, EmbeddingError, ExecutionContext, KagariEngine,
};
use kagari_runtime::value::Value;

fn compile(engine: &KagariEngine, root: FileId, options: CompileOptions) -> BytecodeArtifact {
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root, options, &Default::default())
        .unwrap();
    engine.emit_bytecode(&checked, Default::default()).unwrap()
}

fn insert(engine: &KagariEngine, name: &str, text: &str) -> FileId {
    let source = format!("mem://{name}");
    engine
        .bind_module(
            &source,
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec![name.into()],
            },
        )
        .unwrap();
    engine
        .set_source(&source, text.into(), SourceLayer::Base)
        .unwrap()
}

#[test]
fn reachable_cycle_diagnostics_keep_the_dependency_file_and_revision() {
    let engine = KagariEngine::default();
    let root = insert(&engine, "root", "use pkg::a; fn main() -> i32 { 7 }");
    let a = insert(&engine, "a", "use pkg::b;");
    let b = insert(&engine, "b", "use pkg::a;");
    let sources = engine.source_snapshot();
    let error = engine
        .compile_snapshot(
            sources.clone(),
            root,
            CompileOptions::default(),
            &CancellationToken::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected source diagnostics");
    };
    assert_eq!(diagnostics.len(), 2);
    for diagnostic in diagnostics {
        assert_eq!(diagnostic.code, "KG_RESOLVE_CYCLIC_IMPORT");
        let location = diagnostic.span.unwrap();
        assert!([a, b].contains(&location.file));
        assert!(sources.contains(location));
    }
    let independent = insert(&engine, "independent", "fn main() -> i32 { 42 }");
    assert!(
        engine
            .compile_snapshot(
                engine.source_snapshot(),
                independent,
                CompileOptions::default(),
                &CancellationToken::default()
            )
            .is_ok()
    );
}

#[test]
fn source_and_encoded_programs_execute_transitive_calls_and_shared_struct_layouts() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "shared",
        "pub struct Data { var x: i32 } fn id<T>(x: T) -> T { x } pub fn make(x: i32) -> Data { Data { x: id(x) } } pub fn add(p: Data, x: i32) -> i32 { p.x += x; p.x }",
    );
    insert(&engine, "left", "pub use pkg::shared::make;");
    insert(&engine, "right", "pub use pkg::shared::add;");
    let root = insert(
        &engine,
        "root",
        "use pkg::left::make; use pkg::right::add; fn id() -> bool { true } fn main() -> i32 { val p = make(20); val n = add(p, 22); p.x }",
    );
    let artifact = compile(&engine, root, Default::default());
    assert_eq!(artifact.program.modules.len(), 4);
    assert_eq!(artifact.verification.dependency_fingerprints.len(), 3);
    let encoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    for artifact in [artifact, encoded] {
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
        assert!(loaded.members().all(|member| {
            runtime
                .runtime()
                .module_instance_snapshot(&member)
                .unwrap()
                .state
                == kagari_runtime::ModuleInitializationState::Initialized
        }));
    }
}

#[test]
fn unused_dependency_body_errors_prevent_compilation_with_owned_locations() {
    let engine = KagariEngine::default();
    let dependency = insert(
        &engine,
        "dependency",
        "pub fn good() -> i32 { 42 } fn broken() -> i32 { false }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::dependency; fn main() -> i32 { 7 }",
    );
    let source = engine.source_snapshot();
    let error = engine
        .compile_snapshot(
            source.clone(),
            root,
            Default::default(),
            &Default::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected dependency diagnostics")
    };
    assert!(!diagnostics.is_empty());
    for diagnostic in diagnostics {
        let span = diagnostic.span.unwrap();
        assert_eq!(span.file, dependency);
        assert!(source.contains(span));
    }
}

fn host_fixture(
    failing: bool,
) -> (
    KagariEngine,
    BytecodeArtifact,
    ExecutionContext,
    kagari_common::host_interface::HostFunctionDeclaration,
) {
    use kagari_common::host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle, HostValueType,
    };
    let engine = KagariEngine::default();
    let declaration = HostFunctionDeclaration::new(
        "trace.record",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    engine
        .set_host_interface(HostInterface {
            types: Vec::new(),
            functions: vec![declaration.clone()],
        })
        .unwrap();
    insert(
        &engine,
        "shared",
        if failing {
            "val started = trace::record(1); val broken = 1 / 0; pub fn value() -> i32 { 42 }"
        } else {
            "val started = trace::record(1); pub fn value() -> i32 { 42 }"
        },
    );
    insert(
        &engine,
        "left",
        "use pkg::shared; val started = trace::record(2);",
    );
    insert(
        &engine,
        "right",
        "use pkg::shared; val started = trace::record(3);",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::left; use pkg::right; val started = trace::record(4); fn main() -> i32 { 42 }",
    );
    let context = ExecutionContext {
        language_profile: kagari_runtime::LanguageProfile {
            allow_host_calls: true,
            ..Default::default()
        },
        capabilities: kagari_runtime::CapabilitySet {
            host_calls: true,
            ..Default::default()
        },
        host_policy: kagari_runtime::HostExposurePolicy {
            allowed_host_functions: vec!["trace.record".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let artifact = compile(
        &engine,
        root,
        CompileOptions {
            language_profile: context.language_profile,
        },
    );
    (engine, artifact, context, declaration)
}

#[test]
fn diamond_initialization_is_dependency_first_once_per_runtime_and_failure_is_cached() {
    use std::sync::{Arc, Mutex};
    for failing in [false, true] {
        let (engine, artifact, context, declaration) = host_fixture(failing);
        let calls = Arc::new(Mutex::new(Vec::new()));
        for _ in 0..2 {
            let mut runtime = engine.runtime(context.clone());
            let recorded = calls.clone();
            runtime
                .register_host_function(kagari_runtime::host::HostFunction::new(
                    declaration.clone(),
                    move |_, args| {
                        recorded.lock().unwrap().push(args[0].clone());
                        Ok(args[0].clone())
                    },
                ))
                .unwrap();
            let loaded = runtime
                .load_program(artifact.clone(), Default::default())
                .unwrap();
            for _ in 0..2 {
                let result = runtime.execute(&loaded, "main", &[], &context);
                if failing {
                    assert!(result.is_err());
                } else {
                    assert_eq!(result.unwrap().return_value, Value::I32(42));
                }
            }
            let expected_state = if failing {
                kagari_runtime::ModuleInitializationState::Failed
            } else {
                kagari_runtime::ModuleInitializationState::Initialized
            };
            assert_eq!(
                runtime
                    .runtime()
                    .module_instance_snapshot(&loaded)
                    .unwrap()
                    .state,
                expected_state
            );
        }
        let expected = if failing {
            vec![1, 1]
        } else {
            vec![1, 2, 3, 4, 1, 2, 3, 4]
        };
        assert_eq!(
            *calls.lock().unwrap(),
            expected.into_iter().map(Value::I32).collect::<Vec<_>>()
        );
    }
}

#[test]
fn dependency_bindings_and_execution_policy_are_checked_before_initialization() {
    use std::sync::{Arc, Mutex};
    let (engine, _, context, declaration) = host_fixture(false);
    let root = insert(
        &engine,
        "root",
        "use pkg::left; use pkg::right; fn main() -> i32 { 42 }",
    );
    let artifact = compile(
        &engine,
        root,
        CompileOptions {
            language_profile: context.language_profile,
        },
    );
    assert!(
        artifact.program.modules[artifact.program.root.index()]
            .host_interface
            .functions
            .is_empty()
    );
    let mut runtime = engine.runtime(context.clone());
    assert!(
        runtime
            .load_program(artifact.clone(), Default::default())
            .is_err()
    );
    assert_eq!(runtime.runtime().modules().loaded_count(), 0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    runtime
        .register_host_function(kagari_runtime::host::HostFunction::new(
            declaration,
            move |_, args| {
                recorded.lock().unwrap().push(args[0].clone());
                Ok(args[0].clone())
            },
        ))
        .unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(loaded.epoch.0, 1);
    assert!(
        runtime
            .execute(&loaded, "main", &[], &ExecutionContext::default())
            .is_err()
    );
    assert!(calls.lock().unwrap().is_empty());
    assert!(loaded.members().all(|member| {
        runtime
            .runtime()
            .module_instance_snapshot(&member)
            .unwrap()
            .state
            == kagari_runtime::ModuleInitializationState::Uninitialized
    }));
}

#[test]
fn reload_rejects_same_named_dependency_type_changes_before_publication() {
    let engine = KagariEngine::default();
    for name in ["left", "right"] {
        insert(&engine, name, "pub struct Item { val value: i32 }");
    }
    let mut artifacts = Vec::new();
    for (side, result) in [("left", 42), ("right", 99)] {
        let root = insert(
            &engine,
            "root",
            &format!(
                "use pkg::left; use pkg::right; use pkg::{side}::Item; pub fn expose(value: Item) -> Item {{ value }} fn main() -> i32 {{ {result} }}"
            ),
        );
        artifacts.push(compile(&engine, root, Default::default()));
    }
    for encoded in [false, true] {
        let prepare = |artifact: &BytecodeArtifact| {
            if encoded {
                BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
            } else {
                artifact.clone()
            }
        };
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let active = runtime
            .load_program(prepare(&artifacts[0]), Default::default())
            .unwrap();
        let error = runtime
            .reload_program(&active, prepare(&artifacts[1]), Default::default())
            .unwrap_err();
        assert_eq!(error.code(), "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH");
        assert_eq!(
            runtime
                .execute(&active, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
        // Rejection leaves the baseline active, so a valid reload can still publish.
        runtime
            .reload_program(&active, prepare(&artifacts[0]), Default::default())
            .unwrap();
    }
}

#[test]
fn old_program_calls_keep_their_dependency_versions_after_reload() {
    use kagari_runtime::ModuleEpochRetention;
    let engine = KagariEngine::default();
    insert(&engine, "dependency", "pub fn value() -> i32 { 1 }");
    let root = insert(
        &engine,
        "root",
        "use pkg::dependency::value; fn main() -> i32 { value() }",
    );
    let first = compile(&engine, root, Default::default());
    insert(&engine, "dependency", "pub fn value() -> i32 { 2 }");
    let second = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let old = runtime.load_program(first, Default::default()).unwrap();
    let dependency = old.members().next().unwrap();
    assert!(
        runtime
            .runtime()
            .modules()
            .retain_epoch(dependency.key(), ModuleEpochRetention::ActiveCall)
    );
    let new = runtime
        .reload_program(&old, second.clone(), Default::default())
        .unwrap();
    assert!(
        runtime
            .runtime()
            .modules()
            .collect_unreachable_epochs()
            .is_empty()
    );
    for (module, value) in [(&old, 1), (&new, 2), (&old, 1)] {
        assert_eq!(
            runtime
                .execute(module, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(value)
        );
    }
    assert!(
        runtime
            .reload_program(&old, second, Default::default())
            .is_err()
    );
    assert!(
        runtime
            .runtime()
            .modules()
            .release_epoch(dependency.key(), ModuleEpochRetention::ActiveCall)
    );
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .collect_unreachable_epochs()
            .len(),
        2
    );
    assert_eq!(
        runtime
            .execute(&new, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(2)
    );
}

#[test]
fn malformed_programs_are_rejected_before_any_member_is_published() {
    use kagari_ir::bytecode::{
        BytecodeInstruction, BytecodeModule, CallTarget, FunctionRef, ModuleRef,
    };
    let engine = KagariEngine::default();
    insert(
        &engine,
        "dependency",
        "pub fn number(x: i32) -> i32 { x } pub fn flag(x: bool) -> i32 { if x { 1 } else { 0 } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::dependency::number; fn main() -> i32 { number(42) }",
    );
    let artifact = compile(&engine, root, Default::default());
    let program = &artifact.program;
    let mut malformed = Vec::new();
    let mut bad = program.clone();
    bad.root = ModuleRef::new(100);
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[0].dependencies.push(program.root);
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[program.root.index()].dependencies.clear();
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[program.root.index()]
        .dependencies
        .push(ModuleRef::new(0));
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[0].identity = bad.modules[program.root.index()].identity.clone();
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules.push(BytecodeModule::default());
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[0].module_init = Some(FunctionRef::new(0));
    malformed.push(bad);
    let flag = program.modules[0]
        .functions
        .iter()
        .find(|function| function.name == "flag")
        .unwrap()
        .id;
    for (module, function) in [
        (ModuleRef::new(100), FunctionRef::new(0)),
        (ModuleRef::new(0), FunctionRef::new(100)),
        (ModuleRef::new(0), flag),
    ] {
        let mut bad = program.clone();
        let call = bad.modules[program.root.index()]
            .functions
            .iter_mut()
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| {
                if let BytecodeInstruction::Call {
                    callee: callee @ CallTarget::ModuleFunction { .. },
                    ..
                } = instruction
                {
                    Some(callee)
                } else {
                    None
                }
            })
            .unwrap();
        *call = CallTarget::ModuleFunction { module, function };
        malformed.push(bad);
    }
    let mut runtime = engine.runtime(ExecutionContext::default());
    for bad in malformed {
        assert!(BytecodeArtifact::from_program(bad.clone(), Default::default()).is_err());
        let mut corrupted = artifact.clone();
        corrupted.program = bad;
        let encoded = corrupted.to_bytes().unwrap();
        let decoded = BytecodeArtifact::from_bytes(&encoded).unwrap();
        assert!(runtime.load_program(decoded, Default::default()).is_err());
        assert_eq!(runtime.runtime().modules().loaded_count(), 0);
    }
    assert_eq!(
        runtime
            .load_program(artifact, Default::default())
            .unwrap()
            .epoch
            .0,
        1
    );
}
