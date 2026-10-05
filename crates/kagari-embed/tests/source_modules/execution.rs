use super::*;
use kagari_runtime::session::TraceValue;

use kagari_embed::program::PreparedProgram;
use std::collections::HashSet;

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
        .compile_snapshot(source.clone(), root, &Default::default())
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

#[test]
fn execution_report_records_code_inputs_and_ordered_host_results() {
    let (engine, artifact, mut context, declaration) = host_fixture();
    context.tracing_enabled = true;
    context.inputs.unix_time_millis = 31_415;
    context.inputs.random_seed = 27;
    let decoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut traces = Vec::new();
    for artifact in [artifact, decoded] {
        let mut runtime = engine.runtime(context.clone());
        runtime
            .register_host_function(kagari_runtime::host::HostFunction::new(
                declaration.clone(),
                |_, args| Ok(args[0].clone()),
            ))
            .unwrap();
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = runtime.execute(&loaded, "main", &[], &context).unwrap();
        assert_eq!(
            report
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        let trace = report.trace.unwrap();
        assert_eq!(trace.code_fingerprint, loaded.program_fingerprint());
        assert_eq!(trace.inputs, context.inputs);
        assert_eq!(trace.host_calls.len(), 4);
        let observed = trace
            .host_calls
            .iter()
            .map(|call| (call.arguments.clone(), call.outcome.clone()))
            .collect::<Vec<_>>();
        let expected = [4, 2, 1, 3]
            .into_iter()
            .map(|value| {
                let value = TraceValue::I32(value);
                (vec![value.clone()], Some(Ok(value)))
            })
            .collect::<Vec<_>>();
        assert_eq!(observed, expected);
        traces.push(trace);
    }
    assert_eq!(traces[0], traces[1]);
}

#[test]
fn dependency_bindings_are_checked_before_execution() {
    use std::sync::{Arc, Mutex};
    let (engine, _, context, declaration) = host_fixture();
    let root = insert(
        &engine,
        "root",
        "use pkg::left; use pkg::right; fn main() -> i32 { 42 }",
    );
    let artifact = compile(&engine, root);
    assert!(
        artifact.program.modules[artifact.program.root.index()]
            .host_interface
            .functions
            .is_empty()
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
                Default::default()
            )
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
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(loaded.epoch.0, 1);
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &ExecutionContext::default())
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(calls.lock().unwrap().is_empty());
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
        artifacts.push(compile(&engine, root));
    }
    for encoded in [false, true] {
        let prepare = |artifact: &BytecodeArtifact| {
            let artifact = if encoded {
                BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
            } else {
                artifact.clone()
            };
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap()
        };
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let active = runtime
            .load_program(&prepare(&artifacts[0]), Default::default())
            .unwrap();
        let error = runtime
            .reload_program(&active, &prepare(&artifacts[1]), Default::default())
            .unwrap_err();
        assert_eq!(error.code(), "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH");
        assert_eq!(
            runtime
                .execute(&active, "main", &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        // Rejection leaves the baseline active, so a valid reload can still publish.
        runtime
            .reload_program(&active, &prepare(&artifacts[0]), Default::default())
            .unwrap();
    }
}

#[test]
fn old_program_calls_keep_their_dependency_versions_after_reload() {
    use kagari_runtime::module::ModuleEpochRetention;
    let engine = KagariEngine::default();
    insert(&engine, "dependency", "pub fn value() -> i32 { 1 }");
    let root = insert(
        &engine,
        "root",
        "use pkg::dependency::value; fn main() -> i32 { value() }",
    );
    let first = compile(&engine, root);
    insert(&engine, "dependency", "pub fn value() -> i32 { 2 }");
    let second = compile(&engine, root);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let old_program =
        PreparedProgram::from_artifact(first, &Default::default(), &Default::default()).unwrap();
    let old = runtime
        .load_program(&old_program, Default::default())
        .unwrap();
    let dependency = old
        .members()
        .find(|module| {
            module.bytecode.identity
                == ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["dependency".into()],
                }
        })
        .unwrap();
    let retention = runtime
        .runtime()
        .retain_module(&dependency, ModuleEpochRetention::ActiveCall)
        .unwrap();
    let new = runtime
        .reload_program(
            &old,
            &PreparedProgram::from_artifact(
                second.clone(),
                &Default::default(),
                &Default::default(),
            )
            .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert!(
        runtime
            .runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    for (module, value) in [(&old, 1), (&new, 2), (&old, 1)] {
        assert_eq!(
            runtime
                .execute(module, "main", &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(value)
        );
    }
    assert!(
        runtime
            .reload_program(
                &old,
                &PreparedProgram::from_artifact(second, &Default::default(), &Default::default())
                    .unwrap(),
                Default::default()
            )
            .is_err()
    );
    drop(retention);
    let collected: HashSet<_> = runtime
        .runtime()
        .collect_garbage()
        .unwrap()
        .reclaimed_modules
        .into_iter()
        .collect();
    let old_members: HashSet<_> = old.members().map(|module| module.key()).collect();
    assert_eq!(collected, old_members);
    assert_eq!(
        runtime
            .execute(&new, "main", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(2)
    );
}

#[test]
fn malformed_programs_are_rejected_before_any_member_is_published() {
    use kagari_bytecode::{
        instruction::{BytecodeInstruction, CallTarget},
        module::BytecodeModule,
        program::ModuleRef,
    };
    use kagari_contract::ids::FunctionRef;
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
    let artifact = compile(&engine, root);
    let program = &artifact.program;
    let dependency = ModuleRef::new(
        program
            .modules
            .iter()
            .position(|module| {
                module.identity
                    == ModuleIdentity {
                        package: PackageId("pkg".into()),
                        path: vec!["dependency".into()],
                    }
            })
            .unwrap(),
    );
    let mut malformed = Vec::new();
    let mut bad = program.clone();
    bad.root = ModuleRef::new(100);
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[program.root.index()].dependencies.clear();
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[program.root.index()]
        .dependencies
        .push(program.modules[program.root.index()].dependencies[0]);
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[dependency.index()].identity = bad.modules[program.root.index()].identity.clone();
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules.push(BytecodeModule::default());
    malformed.push(bad);
    let flag = program.modules[dependency.index()]
        .functions
        .iter()
        .find(|function| function.name == "flag")
        .unwrap()
        .id;
    for (module, function) in [
        (ModuleRef::new(100), FunctionRef::new(0)),
        (dependency, FunctionRef::new(100)),
        (dependency, flag),
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
        assert!(
            PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default())
                .is_err()
        );
        assert_eq!(runtime.runtime().modules().loaded_count(), 0);
    }
    assert_eq!(
        runtime
            .load_program(
                &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                    .unwrap(),
                Default::default()
            )
            .unwrap()
            .epoch
            .0,
        1
    );
}
