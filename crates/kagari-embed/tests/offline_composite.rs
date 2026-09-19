use kagari_common::{
    SourceFile,
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle,
        HostValueType as Type,
    },
};
use kagari_embed::{
    ArtifactOptions, CompileOptions, ExecutionContext, HostExposurePolicy, KagariEngine,
    LoadOptions,
};
use kagari_runtime::{
    CapabilitySet, LanguageProfile,
    host::HostFunction,
    value::{EnumTag, Value},
};
use std::sync::{Arc, Mutex};

fn composite() -> Type {
    Type::Tuple(vec![
        Type::Array(Box::new(Type::I32)),
        Type::Map {
            key: Box::new(Type::String),
            value: Box::new(Type::Bool),
        },
        Type::Set(Box::new(Type::String)),
        Type::Option(Box::new(Type::I32)),
        Type::Result {
            ok: Box::new(Type::I32),
            error: Box::new(Type::String),
        },
    ])
}

#[test]
fn offline_composite_calls_preserve_shapes_and_gc_roots_across_execution_routes() {
    let make = HostFunctionDeclaration::new("demo.make", vec![], composite());
    let echo = HostFunctionDeclaration::new(
        "demo.echo",
        vec![HostParameter {
            name: "value".into(),
            ty: composite(),
            passing: HostPassingStyle::Owned,
        }],
        composite(),
    );
    let interface = HostInterface {
        functions: vec![make.clone(), echo.clone()],
    };
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface::from_bytes(&interface.to_bytes().unwrap()).unwrap())
        .unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_jit: true,
        ..Default::default()
    };
    let artifact = engine.compile_to_artifact(SourceFile::new("composite.kgr", "use demo as api; fn main() -> ([i32], Map<String, bool>, Set<String>, Option<i32>, Result<i32, String>) { api::echo(api::make()) }"), CompileOptions { language_profile: profile }, ArtifactOptions::default()).unwrap();
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
                allowed_host_functions: vec!["demo.make".into(), "demo.echo".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let trace = Arc::new(Mutex::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(make.clone(), move |context, _| {
                calls.lock().unwrap().push("make");
                let runtime = context.runtime();
                Ok(Value::Tuple(vec![
                    Value::Array(runtime.alloc_array(vec![Value::I32(7)]).unwrap()),
                    Value::Map(
                        runtime
                            .alloc_map(vec![(Value::Str("yes".into()), Value::Bool(true))])
                            .unwrap(),
                    ),
                    Value::Set(runtime.alloc_set(vec![Value::Str("name".into())]).unwrap()),
                    Value::Enum(
                        runtime
                            .alloc_enum(EnumTag::OptionSome, vec![Value::I32(8)])
                            .unwrap(),
                    ),
                    Value::Enum(
                        runtime
                            .alloc_enum(EnumTag::ResultOk, vec![Value::I32(9)])
                            .unwrap(),
                    ),
                ]))
            }))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(echo.clone(), move |context, args| {
                calls.lock().unwrap().push("echo");
                context.runtime().collect_garbage().unwrap();
                Ok(args[0].clone())
            }))
            .unwrap();
        let loaded = runtime
            .load_program(artifact, LoadOptions::default())
            .unwrap();
        assert!(trace.lock().unwrap().is_empty());
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        let retained = runtime.runtime().root_value(report.return_value).unwrap();
        runtime.runtime().collect_garbage().unwrap();
        let Value::Tuple(values) = retained.value() else {
            panic!("composite return")
        };
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
        assert_eq!(
            heap.map_snapshot(*map).unwrap(),
            [(Value::Str("yes".into()), Value::Bool(true))]
        );
        assert_eq!(
            heap.set_snapshot(*set).unwrap(),
            [Value::Str("name".into())]
        );
        assert_eq!(heap.enum_snapshot(*option).unwrap().fields, [Value::I32(8)]);
        assert_eq!(heap.enum_snapshot(*result).unwrap().fields, [Value::I32(9)]);
        assert_eq!(*trace.lock().unwrap(), ["make", "echo"]);
    }
}

#[test]
fn offline_composite_signatures_reject_nested_source_mismatches() {
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface {
            functions: vec![HostFunctionDeclaration::new(
                "demo.take",
                vec![HostParameter {
                    name: "value".into(),
                    ty: Type::Tuple(vec![Type::Array(Box::new(Type::I32)), Type::Bool]),
                    passing: HostPassingStyle::Owned,
                }],
                Type::Unit,
            )],
        })
        .unwrap();
    let error = engine
        .compile_source(
            SourceFile::new("bad.kgr", "fn main() { demo::take(([true], true)); }"),
            CompileOptions {
                language_profile: LanguageProfile {
                    allow_host_calls: true,
                    ..Default::default()
                },
            },
        )
        .unwrap_err();
    assert!(format!("{error:?}").contains("KG_TYPE_ARGUMENT_TYPE_MISMATCH"));
}
