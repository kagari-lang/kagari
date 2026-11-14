use kagari::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::Program,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::{
    error::RuntimeErrorKind,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        declarations::{FunctionDecl, MethodDecl},
        payload::{
            Opaque,
            managed::{Field, Managed, ManagedStorage},
        },
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    value::view::ValueView,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use rstest::rstest;

type Boxed = Opaque<Managed<()>>;

fn store_value(
    cx: &mut NativeContext<'_>,
    field: &Field<ScriptValue>,
    value: ScriptValue,
) -> NativeResult<Boxed> {
    let ty = cx.result_native_type::<Managed<()>>()?;
    let field = ty.bind_field(cx.runtime_mut(), field)?;
    let mut builder = ty.build(())?;
    builder.set(cx, &field, value)?;
    builder.finish(cx)
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
    let parameter = boxed.type_parameter("T").unwrap();
    let mut storage = ManagedStorage::<()>::new();
    let field = storage
        .field::<ScriptValue>("value", parameter.ty())
        .unwrap();
    boxed.native_storage(storage.finish()).unwrap();
    let boxed = boxed.finish().unwrap();
    let stored_field = field.clone();
    module
        .implement(boxed.clone(), |group| {
            let parameter = group.parameter("T")?.ty();
            group.trait_impl(item.apply([parameter])?, |methods| {
                methods.bind(
                    "value",
                    move |cx: &mut NativeContext<'_>,
                          (stored,): (Boxed,)|
                          -> NativeResult<ScriptValue> {
                        let field = stored
                            .native_type()
                            .bind_field(cx.runtime_mut(), &stored_field)?;
                        stored.get(cx, &field)
                    },
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
    let singleton_field = field.clone();
    module
        .bind(
            singleton,
            move |cx: &mut NativeContext<'_>, (value,): (ScriptValue,)| -> NativeResult<Boxed> {
                store_value(cx, &singleton_field, value)
            },
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
            NativeBinding::new([], Codec::Value, |_| Ok(ValueView::from(42_i32))),
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
            move |cx: &mut NativeContext<'_>,
                  (_receiver, value): (ScriptValue, ScriptValue)|
                  -> NativeResult<Boxed> { store_value(cx, &field, value) },
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
    include_str!("../fixtures/embedding/native-results/generic-scalar-interface.kgr")
)]
#[case::nested_array_function(
    include_str!("../fixtures/embedding/native-results/nested-array-function.kgr")
)]
#[case::nested_array_method(
    include_str!("../fixtures/embedding/native-results/nested-array-method.kgr")
)]
#[case::distinct_nominal_results(
    include_str!("../fixtures/embedding/native-results/distinct-nominal-results.kgr")
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
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    let replacement = runtime
        .program(&loaded)
        .unwrap()
        .prepare_replacement(&program, &Default::default())
        .unwrap();
    runtime
        .replace_program(&loaded, &replacement, Default::default())
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
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
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(
        error.to_string(),
        "module validation failed: converted value differs from its declaration"
    );
    assert_eq!(error.trace().unwrap().frames[0].function_name, "wrong");
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
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
            NativeBinding::new([], Codec::Value, |_| Ok(ValueView::from(42_i32))),
        )
        .unwrap();
    assert!(module.finish().is_err());
}
