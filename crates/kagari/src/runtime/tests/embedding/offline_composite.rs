#[cfg(feature = "native")]
use crate::program::native::NativeCompiler;
use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::{KagariEngine, source::ArtifactOptions},
    program::Compilation,
    runtime::LoadOptions,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_runtime::execution::reentry::reenter;
use kagari_runtime::{
    host::HostError,
    module::entry::ProgramEntry,
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::FunctionDecl,
        host, types::Type,
    },
    value::view::ValueView,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
    ty::Ty,
};
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
        Type::tuple([string.clone(), Type::bool()]).unwrap(),
        string.clone(),
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
        .bind(
            make,
            host::callback(move |context, _| {
                calls.lock().unwrap().push("make");
                let owner = context.runtime().execution_root().unwrap();
                let payload = owner
                    .bytecode
                    .functions
                    .iter()
                    .find(|function| function.name == "payload")
                    .ok_or_else(|| HostError::new("missing pinned payload entry"))?
                    .id;
                let arguments = context.retain_temporaries(&[]).unwrap();
                let payload = reenter(context, &owner, payload, arguments)
                    .map_err(|error| HostError::new(format!("payload construction: {error:?}")))?;
                context
                    .import(&payload)
                    .map_err(|error| HostError::new(error.to_string()))
            }),
        )
        .unwrap();
    let calls = trace.clone();
    builder
        .bind(
            echo,
            host::callback(move |context, args| {
                calls.lock().unwrap().push("echo");
                context.runtime_mut().collect_garbage().unwrap();
                Ok(args[0])
            }),
        )
        .unwrap();
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        stdlib::modules()
            .unwrap()
            .into_iter()
            .chain(vec![builder.finish().unwrap()])
            .collect(),
    )
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
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        trace.lock().unwrap().clear();
        let loaded_program =
            Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let loaded = runtime
            .load_program(loaded_program.program(), LoadOptions::default())
            .unwrap();
        assert!(trace.lock().unwrap().is_empty());
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = NativeCompiler::default()
                .prepare(
                    &loaded_program,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        let retained = report.return_value;
        runtime.collect_garbage().unwrap();
        let owner = runtime.select_program(&loaded).unwrap();
        let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
        let tuple = cx.import(&retained).unwrap();
        assert_eq!(cx.tuple_len(&tuple).unwrap(), 5);
        let array = cx.tuple_element(&tuple, 0).unwrap();
        assert_eq!(cx.array_len(&array).unwrap(), 1);
        assert_eq!(
            cx.array_element(&array, 0).unwrap().unwrap().as_i32(),
            Some(7)
        );
        let record = cx.tuple_element(&tuple, 1).unwrap();
        let ty = cx.type_for::<(String, bool)>().unwrap();
        assert_eq!(
            cx.decode_value::<(String, bool)>(&ty, &record).unwrap(),
            ("yes".to_owned(), true)
        );
        let name = cx.tuple_element(&tuple, 2).unwrap();
        let ty = cx.type_for::<String>().unwrap();
        assert_eq!(cx.decode_value::<String>(&ty, &name).unwrap(), "name");
        let option = cx.tuple_element(&tuple, 3).unwrap();
        let ty = cx.type_for::<Option<i32>>().unwrap();
        assert_eq!(
            cx.decode_value::<Option<i32>>(&ty, &option).unwrap(),
            Some(8)
        );
        let result = cx.tuple_element(&tuple, 4).unwrap();
        let ty = cx.type_for::<Result<i32, String>>().unwrap();
        assert_eq!(
            cx.decode_value::<Result<i32, String>>(&ty, &result)
                .unwrap(),
            Ok(9)
        );
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
        .bind(
            take,
            host::callback(|_, _| panic!("invalid source cannot invoke a host callback")),
        )
        .unwrap();
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        stdlib::modules()
            .unwrap()
            .into_iter()
            .chain(vec![builder.finish().unwrap()])
            .collect(),
    )
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
            host::callback(move |context, args| {
                let owner = context.runtime().execution_root().unwrap();
                *calls.lock().unwrap() += 1;
                context
                    .convert_value(&owner, |cx| {
                        assert_eq!(cx.array_len(&args[0])?, 0);
                        Ok(ValueView::from(42_i32))
                    })
                    .map_err(|error| HostError::new(error.to_string()))
            }),
        )
        .unwrap();
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        stdlib::modules()
            .unwrap()
            .into_iter()
            .chain(vec![builder.finish().unwrap()])
            .collect(),
    )
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
                Compilation::from_artifact(artifact, &Default::default(), &Default::default())
                    .unwrap();
            let loaded = runtime
                .load_program(loaded_program.program(), LoadOptions::default())
                .unwrap();
            let report = if jit {
                let mut backend = CraneliftBackend::for_host().unwrap();
                let prepared = {
                    let native_code = NativeCompiler::default().prepare(
                        &loaded_program,
                        ProgramEntry::Main,
                        &mut backend,
                        &context.cancellation,
                    );
                    native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                }
                .unwrap();
                runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
            } else {
                runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
            }
            .unwrap();
            assert_eq!(report.return_value.as_i32().unwrap(), 42);
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
