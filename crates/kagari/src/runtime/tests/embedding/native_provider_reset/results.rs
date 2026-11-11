use crate::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::Program,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::{
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        declarations::{FunctionDecl, MethodDecl},
        storage::{NativePayload, NativeStorage},
        types::Type,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use rstest::rstest;

#[derive(Debug)]
struct StoredValue(Value);

impl NativePayload for StoredValue {
    fn trace<'a>(&'a self, visit: &mut dyn FnMut(&'a Value)) {
        visit(&self.0);
    }

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(1, 0))
    }
}

fn engine() -> KagariEngine {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::results",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let mut item = module.define_trait("Item").unwrap();
    let parameter = item.type_parameter("T").unwrap().ty();
    item.define_method(MethodDecl::instance("value").returns(parameter))
        .unwrap();
    let item = item.finish().unwrap();
    let mut boxed = module.define_type("Box").unwrap();
    boxed.type_parameter("T").unwrap();
    boxed
        .native_storage(NativeStorage::payload::<StoredValue>())
        .unwrap();
    let boxed = boxed.finish().unwrap();
    module
        .implement(boxed.clone(), |group| {
            let parameter = group.parameter("T")?.ty();
            group.trait_impl(item.apply([parameter])?, |methods| {
                methods.bind(
                    "value",
                    NativeBinding::new([Codec::Value], Codec::Value, |call| {
                        call.with_payload::<StoredValue, _>(0, |stored| Ok(stored.0))
                    }),
                )
            })
        })
        .unwrap();
    let singleton = module
        .define_function(FunctionDecl::new("singleton"))
        .unwrap();
    module
        .function(&singleton, |f| {
            let t = f.type_parameter("T")?.ty();
            f.parameter("value", t.clone())?;
            f.returns(item.apply([t.clone()])?.ty())?;
            f.produces(boxed.apply([t])?)?;
            Ok(())
        })
        .unwrap();
    module
        .bind(
            singleton,
            NativeBinding::new([Codec::Value], Codec::Value, |call| {
                call.allocate_result_payload(StoredValue(call.argument(0)?), &[])
            }),
        )
        .unwrap();
    let wrong = module
        .define_function(
            FunctionDecl::new("wrong").returns(item.apply([Type::i32()]).unwrap().ty()),
        )
        .unwrap();
    module
        .function(&wrong, |f| {
            f.produces(boxed.apply([Type::i32()])?)?;
            Ok(())
        })
        .unwrap();
    module
        .bind(
            wrong,
            NativeBinding::new([], Codec::Value, |_| Ok(Value::I32(42))),
        )
        .unwrap();
    let mut factory = module.define_trait("Factory").unwrap();
    let one = factory.define_method(MethodDecl::instance("one")).unwrap();
    factory
        .method(&one, |f| {
            let t = f.type_parameter("T")?.ty();
            f.parameter("value", t.clone())?;
            f.returns(item.apply([t.clone()])?.ty())?;
            f.produces(boxed.apply([t])?)?;
            Ok(())
        })
        .unwrap();
    factory
        .bind(
            one,
            NativeBinding::new([Codec::Value, Codec::Value], Codec::Value, |call| {
                call.allocate_result_payload(StoredValue(call.argument(1)?), &[])
            }),
        )
        .unwrap();
    factory.finish().unwrap();
    let module = module.finish().unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    {
        let mut builder = KagariEngine::builder().unwrap();
        builder.install_all(stdlib::modules().unwrap()).unwrap();
        builder.config(config);
        builder.install(module).unwrap();
        builder.build().unwrap()
    }
}

fn artifact(engine: &KagariEngine, source: &str) -> DefinitionMetadata<KbcArtifact> {
    engine
        .compile_to_artifact(
            SourceFile::new("memory://native-result.kgr", source),
            Default::default(),
        )
        .unwrap()
}

#[rstest]
#[case::generic_scalar_interface(
    "use example::results::Factory; struct F {} impl Factory for F {} fn main() -> i32 { val f: Factory = F {}; if f.one(1).value() == f.one(1).value() {42} else {0} }"
)]
#[case::nested_array_function(
    "use example::results::singleton; fn main() -> i32 { singleton([42]).value()[0u64] }"
)]
#[case::nested_array_method(
    "use example::results::Factory; struct F {} impl Factory for F {} fn main() -> i32 { val f: Factory = F {}; f.one([42]).value()[0u64] }"
)]
#[case::distinct_nominal_results(
    "use example::results::Factory; struct F {} struct Payload { val value: i32 } impl Factory for F {} fn main() -> i32 { val f: Factory = F {}; val a = f.one(20); val b = f.one(Payload { value: 22 }); a.value() + b.value().value }"
)]
fn concrete_native_results_construct_declared_interfaces_in_generic_calls(#[case] source: &str) {
    let engine = engine();
    let original = artifact(&engine, source);
    assert!(
        original
            .records()
            .program
            .modules
            .iter()
            .flat_map(|m| &m.native_imports)
            .any(|import| import.result_adapter.is_some())
    );
    let decoded =
        KbcArtifact::from_bytes(&original.records().to_bytes(original.definitions()).unwrap())
            .unwrap();
    let program =
        Program::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc().allocated_objects(), 0);
}

#[test]
fn forged_native_result_conversions_are_rejected() {
    let engine = engine();
    let original = artifact(
        &engine,
        "use example::results::singleton; fn main() -> i32 { singleton(42).value() }",
    );
    let context = DefinitionContext::new().unwrap();
    let original = original
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let definitions = context.snapshot();
    for corruption in 0..5 {
        let mut program = original.program.clone();
        let import = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.native_imports)
            .find(|import| {
                import.result_adapter.is_some()
                    && definitions
                        .resolve(import.instance.declaration)
                        .unwrap()
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
            3 => {
                let id = &mut import
                    .result_adapter
                    .as_mut()
                    .unwrap()
                    .implementation
                    .declaration;
                let parent = definitions.parent(*id).unwrap().unwrap();
                let segment = definitions.resolve(*id).unwrap().last().unwrap();
                *id = context
                    .intern_child(
                        parent,
                        segment.kind,
                        &format!("{}_missing", segment.name),
                        segment.occurrence,
                    )
                    .unwrap();
            }
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
            KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
                .is_err(),
            "corruption {corruption}"
        );
    }
}

#[test]
fn native_result_conversion_validates_the_rust_body_value() {
    let engine = engine();
    let program = Program::from_artifact(
        artifact(
            &engine,
            "use example::results::wrong; fn main() -> i32 { wrong().value() }",
        ),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .is_err()
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    assert!(!runtime.is_quarantined());
}

#[test]
fn native_registration_rejects_an_unimplemented_result_interface() {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::bad_result",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let function = module
        .define_function(
            FunctionDecl::new("wrong").returns(language.list().apply([Type::i32()]).unwrap().ty()),
        )
        .unwrap();
    module
        .function(&function, |f| {
            f.produces(Type::i32())?;
            Ok(())
        })
        .unwrap();
    module
        .bind(
            function,
            NativeBinding::new([], Codec::Value, |_| Ok(Value::I32(42))),
        )
        .unwrap();
    assert!(module.finish().is_err());
}
