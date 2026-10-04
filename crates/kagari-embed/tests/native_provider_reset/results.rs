use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::{FunctionDecl, MethodDecl},
        types::Type,
    },
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn engine() -> KagariEngine {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::results",
        &language.catalog().expect("explicit standard providers"),
    );
    let singleton = module
        .define_function(FunctionDecl::new("singleton"))
        .unwrap();
    module
        .function(&singleton, |f| {
            let t = f.type_parameter("T")?.ty();
            f.parameter("value", t.clone());
            f.returns(language.list().apply([t.clone()]).ty());
            f.produces(language.vec(t));
            Ok(())
        })
        .unwrap();
    module
        .bind_with(
            singleton,
            NativeBinding::new([Codec::Value], Codec::MutableSequence, |call| {
                call.allocate_sequence(call.result_type_parameter(0)?, vec![call.argument(0)?])
            }),
        )
        .unwrap();
    let wrong = module
        .define_function(
            FunctionDecl::new("wrong").returns(language.list().apply([Type::i32()]).ty()),
        )
        .unwrap();
    module
        .function(&wrong, |f| {
            f.produces(language.vec(Type::i32()));
            Ok(())
        })
        .unwrap();
    module
        .bind_with(
            wrong,
            NativeBinding::new([], Codec::Value, |_| Ok(Value::I32(42))),
        )
        .unwrap();
    let mut factory = module.define_trait("Factory");
    let one = factory.define_method(MethodDecl::instance("one")).unwrap();
    factory
        .method(&one, |f| {
            let t = f.type_parameter("T")?.ty();
            f.parameter("value", t.clone());
            f.returns(language.list().apply([t.clone()]).ty());
            f.produces(language.vec(t));
            Ok(())
        })
        .unwrap();
    factory
        .bind_default_with(
            one,
            NativeBinding::new(
                [Codec::Value, Codec::Value],
                Codec::MutableSequence,
                |call| {
                    call.allocate_sequence(call.result_type_parameter(0)?, vec![call.argument(1)?])
                },
            ),
        )
        .unwrap();
    factory.finish().unwrap();
    let module = module.finish().unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install(module)
        .build()
        .unwrap()
}

fn artifact(engine: &KagariEngine, source: &str) -> KbcArtifact {
    engine
        .compile_to_artifact(
            SourceFile::new("memory://native-result.kgr", source),
            Default::default(),
        )
        .unwrap()
}

#[test]
fn concrete_native_results_construct_declared_interfaces_in_generic_calls() {
    let engine = engine();
    for source in [
        "use example::results::singleton; fn main() -> i32 { singleton([42])[0usize][0usize] }",
        "use example::results::Factory; struct F {} impl Factory for F {} fn main() -> i32 { val f: Factory = F {}; f.one([42])[0usize][0usize] }",
        "use example::results::Factory; struct F {} struct Payload { val value: i32 } impl Factory for F {} fn main() -> i32 { val f: Factory = F {}; val a = f.one(20); val b = f.one(Payload { value: 22 }); a[0usize] + b[0usize].value }",
    ] {
        let original = artifact(&engine, source);
        assert!(
            original
                .program
                .modules
                .iter()
                .flat_map(|m| &m.native_imports)
                .any(|import| import.result_adapter.is_some())
        );
        let decoded = KbcArtifact::from_bytes(&original.to_bytes().unwrap()).unwrap();
        let program =
            PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn forged_native_result_conversions_are_rejected() {
    let engine = engine();
    let original = artifact(
        &engine,
        "use example::results::singleton; fn main() -> i32 { singleton(42)[0usize] }",
    );
    for corruption in 0..5 {
        let mut program = original.program.clone();
        let import = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.native_imports)
            .find(|import| {
                import.result_adapter.is_some()
                    && import
                        .instance
                        .declaration
                        .path
                        .last()
                        .is_some_and(|part| part.name == "singleton")
            })
            .unwrap();
        match corruption {
            0 => import.result_adapter = None,
            1 => import.result_adapter.as_mut().unwrap().receiver = Ty::Builtin(BuiltinType::I32),
            2 => import
                .result_adapter
                .as_mut()
                .unwrap()
                .implementation
                .arguments
                .clear(),
            3 => import
                .result_adapter
                .as_mut()
                .unwrap()
                .implementation
                .declaration
                .path
                .last_mut()
                .unwrap()
                .name
                .push_str("_missing"),
            _ => {
                let selected = import
                    .result_adapter
                    .as_ref()
                    .unwrap()
                    .implementation
                    .clone();
                for module in &mut program.modules {
                    module.interface_tables.retain(|table| {
                        table.declaration != selected.declaration
                            || table.arguments != selected.arguments
                    });
                }
            }
        }
        assert!(
            KbcArtifact::from_program(program, Default::default()).is_err(),
            "corruption {corruption}"
        );
    }
}

#[test]
fn native_result_conversion_validates_the_rust_body_value() {
    let engine = engine();
    let program = PreparedProgram::from_artifact(
        artifact(
            &engine,
            "use example::results::wrong; fn main() -> i32 { wrong()[0usize] }",
        ),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert!(runtime.execute(&loaded, "main", &[], &context).is_err());
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert!(!runtime.runtime().is_quarantined());
}

#[test]
fn native_registration_rejects_an_unimplemented_result_interface() {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::bad_result",
        &language.catalog().expect("explicit standard providers"),
    );
    let function = module
        .define_function(
            FunctionDecl::new("wrong").returns(language.list().apply([Type::i32()]).ty()),
        )
        .unwrap();
    module
        .function(&function, |f| {
            f.produces(Type::i32());
            Ok(())
        })
        .unwrap();
    module
        .bind_with(
            function,
            NativeBinding::new([], Codec::Value, |_| Ok(Value::I32(42))),
        )
        .unwrap();
    assert!(module.finish().is_err());
}
