use crate::{
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    program::PreparedProgram,
    runtime::LoadOptions,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::{
    host::{HostCallContext, HostError},
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_vm::reentry::reenter;
use std::sync::{Arc, Mutex};

fn array() -> Type {
    Type::from_semantic(
        Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
        StandardDeclarations::default().declarations().unwrap()[0].definitions(),
    )
    .unwrap()
}

fn composite(standard: &StandardDeclarations) -> Type {
    let string = Type::scalar(BuiltinType::String);
    Type::tuple([
        array(),
        standard.hash_map(string.clone(), Type::bool()).unwrap(),
        standard.hash_set(string.clone()).unwrap(),
        standard.option(Type::i32()).unwrap(),
        StandardDeclarations::enumeration("Result")
            .unwrap()
            .apply([Type::i32(), string])
            .unwrap(),
    ])
    .unwrap()
}

#[test]
fn offline_composite_calls_preserve_shapes_and_gc_roots_across_execution_routes() {
    let standard = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new("demo", &standard.catalog().unwrap()).unwrap();
    let shape = composite(&standard);
    let make = builder
        .define_function(
            FunctionDecl::new("make")
                .returns(shape.clone())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        may_call_host_services: true,
                        may_trap: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    let echo = builder
        .define_function(
            FunctionDecl::new("echo")
                .parameter("value", shape.clone())
                .returns(shape)
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::Owned],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let calls = trace.clone();
    builder
        .bind(make, move |context: &HostCallContext<'_>, _: &[Value]| {
            calls.lock().unwrap().push("make");
            let owner = context.runtime().execution_root().unwrap();
            let payload = owner
                .bytecode
                .functions
                .iter()
                .find(|function| function.name == "payload")
                .ok_or_else(|| HostError::new("missing pinned payload entry"))?
                .id;
            let payload = reenter(context, &owner, payload, &[])
                .map_err(|error| HostError::new(format!("payload construction: {error:?}")))?;
            Ok(payload.value(context.runtime().gc()).unwrap())
        })
        .unwrap();
    let calls = trace.clone();
    builder
        .bind(
            echo,
            move |context: &HostCallContext<'_>, args: &[Value]| {
                calls.lock().unwrap().push("echo");
                context.runtime().collect_garbage().unwrap();
                Ok(args[0])
            },
        )
        .unwrap();
    let engine =
        KagariEngine::with_native_modules(Default::default(), vec![builder.finish().unwrap()])
            .unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "composite.kgr",
                include_str!("../../../../tests/fixtures/embedding/host/composite-callbacks.kgr"),
            ),
            ArtifactOptions::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        trace.lock().unwrap().clear();
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, LoadOptions::default())
            .unwrap();
        assert!(trace.lock().unwrap().is_empty());
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        let retained = report.return_value;
        runtime.runtime().collect_garbage().unwrap();
        let Value::Tuple(values) = retained.value(runtime.runtime().gc()).unwrap() else {
            panic!("composite return")
        };
        let values = runtime.runtime().gc().tuple(values).unwrap().to_vec();
        let [
            Value::Array(array),
            Value::Map(map),
            Value::Set(set),
            Value::Enum(option),
            Value::Enum(result),
        ] = values.as_slice()
        else {
            panic!("composite shapes")
        };
        let heap = runtime.runtime().gc();
        assert_eq!(heap.array_snapshot(*array).unwrap(), [Value::I32(7)]);
        let entries = heap.map_snapshot(*map).unwrap();
        let [(Value::Str(key), Value::Bool(true))] = entries.as_slice() else {
            panic!("map contents")
        };
        assert_eq!(&*heap.string(*key).unwrap(), "yes");
        let entries = heap.set_snapshot(*set).unwrap();
        let [Value::Str(key)] = entries.as_slice() else {
            panic!("set contents")
        };
        assert_eq!(&*heap.string(*key).unwrap(), "name");
        assert_eq!(heap.enum_snapshot(*option).unwrap().fields, [Value::I32(8)]);
        assert_eq!(heap.enum_snapshot(*result).unwrap().fields, [Value::I32(9)]);
        assert_eq!(*trace.lock().unwrap(), ["make", "echo"]);
    }
}

#[test]
fn offline_composite_signatures_reject_nested_source_mismatches() {
    let mut builder =
        ModuleBuilder::new("demo", &StandardDeclarations::default().catalog().unwrap()).unwrap();
    let take = builder
        .define_function(
            FunctionDecl::new("take")
                .parameter("value", Type::tuple([array(), Type::bool()]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::Owned],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    builder
        .bind(take, |_: &HostCallContext<'_>, _: &[Value]| {
            panic!("invalid source cannot invoke a host callback")
        })
        .unwrap();
    let engine =
        KagariEngine::with_native_modules(Default::default(), vec![builder.finish().unwrap()])
            .unwrap();
    let error = engine
        .compile_source(SourceFile::new(
            "bad.kgr",
            "use demo; fn main() { demo::take(([true], true)); }",
        ))
        .unwrap_err();
    assert!(format!("{error:?}").contains("KG_TYPE_ARGUMENT_TYPE_MISMATCH"));
}

#[test]
fn offline_host_parameters_supply_context_and_skip_calls_after_terminating_operands() {
    let mut builder =
        ModuleBuilder::new("demo", &StandardDeclarations::default().catalog().unwrap()).unwrap();
    let take = builder
        .define_function(
            FunctionDecl::new("take")
                .parameter("value", array())
                .returns(Type::i32())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::Owned],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    let count = Arc::new(Mutex::new(0));
    let calls = count.clone();
    builder
        .bind(
            take,
            move |context: &HostCallContext<'_>, args: &[Value]| {
                let Value::Array(array) = args[0] else {
                    panic!("array argument")
                };
                assert!(
                    context
                        .runtime()
                        .gc()
                        .array_snapshot(array)
                        .unwrap()
                        .is_empty()
                );
                *calls.lock().unwrap() += 1;
                Ok(Value::I32(42))
            },
        )
        .unwrap();
    let engine =
        KagariEngine::with_native_modules(Default::default(), vec![builder.finish().unwrap()])
            .unwrap();
    for (argument, expected_calls) in [
        ("[]", 1),
        ("if true { [] } else { [] }", 1),
        ("if true { return 42; } else { return 7; }", 0),
    ] {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "host-context.kgr",
                    format!("use demo; fn main() -> i32 {{ demo::take({argument}) }}"),
                ),
                ArtifactOptions::default(),
            )
            .unwrap();
        for (encoded, jit) in [(false, false), (true, false), (true, true)] {
            let artifact = if encoded {
                KbcArtifact::from_bytes(
                    &artifact.records().to_bytes(artifact.definitions()).unwrap(),
                )
                .unwrap()
            } else {
                artifact.clone()
            };
            let context = ExecutionContext {
                ..Default::default()
            };
            let mut runtime = engine.runtime(context.clone());
            *count.lock().unwrap() = 0;
            let loaded_program =
                PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                    .unwrap();
            let loaded = runtime
                .load_program(&loaded_program, LoadOptions::default())
                .unwrap();
            let report = if jit {
                let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
                let prepared = runtime
                    .prepare_native(
                        &loaded_program,
                        &loaded,
                        ProgramEntry::Main,
                        &mut backend,
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
                Value::I32(42)
            );
            assert_eq!(*count.lock().unwrap(), expected_calls);
        }
    }
    for argument in [
        "[true]",
        "if true { return 42; } else { false }",
        "if true { return false; } else { return 7; }",
        "[], missing",
    ] {
        assert!(
            engine
                .compile_source(SourceFile::new(
                    "host-context-invalid.kgr",
                    format!("use demo; fn main() -> i32 {{ demo::take({argument}) }}")
                ),)
                .is_err(),
            "{argument}"
        );
    }
}
