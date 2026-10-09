use kagari_runtime::value_semantics::script_equal;
mod vector_edits;
use super::compile_program;
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, PackageId},
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{GcHeapConfig, roots::RootedValue},
    module::LoadedModule,
    native::{
        binding::{NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        collections::vector::ScriptVec,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        declarations::MethodDecl,
        module::NativeModule,
        objects::Object,
        registration::FunctionSpec,
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::declaration::module::ModuleDecl;
use kagari_vm::vm::Vm;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn script_objects_cross_typed_entries_without_exposing_heap_ids() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let module = ModuleBuilder::new("example::typed", &catalog)
        .finish()
        .unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        pub struct Player { pub var hp: i32 }
        pub fn make() -> Player { Player { hp: 100 } }
        pub fn damage(player: Player, damage: i32) -> i32 { player.hp -= damage; player.hp }
    "#,
        &module,
    );
    let player: Object = vm.execute_typed(&loaded, "make", ()).unwrap();
    let hp = vm
        .runtime()
        .bind_field::<i32>(player.object_type(), "hp")
        .unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(&loaded, "damage", (player.clone(), 15i32))
            .unwrap(),
        85
    );
    vm.runtime().collect_garbage().unwrap();
    let mut cx = NativeContext::new(vm.runtime(), &loaded).unwrap();
    assert_eq!(player.get(&mut cx, &hp).unwrap(), 85);
    player.set(&mut cx, &hp, 42).unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(&loaded, "damage", (player.clone(), 2i32))
            .unwrap(),
        40
    );
    drop(player);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn ordinary_native_callbacks_mutate_retained_collections_through_typed_handles() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("example::typed", &catalog);
    module
        .add_function(
            FunctionSpec::new("append").parameter_names(["values"]),
            |cx: &mut NativeContext<'_>,
             (values,): (ScriptVec<i32>,)|
             -> NativeResult<ScriptVec<i32>> {
                values.push(cx, 42)?;
                cx.collect_garbage()?;
                Ok(values)
            },
        )
        .unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        use example::typed::append;
        pub fn main() -> Vec<i32> { val values = [1, 2]; append(values); values }
    "#,
        &module.finish().unwrap(),
    );
    let values: ScriptVec<i32> = vm.execute_typed(&loaded, "main", ()).unwrap();
    vm.runtime().collect_garbage().unwrap();
    let mut cx = NativeContext::new(vm.runtime(), &loaded).unwrap();
    assert_eq!(values.len(&cx).unwrap(), 3);
    assert_eq!(values.get(&mut cx, 2).unwrap(), Some(42));
    drop(values);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn typed_instance_method_receives_its_receiver_separately_from_tuple_arguments() {
    struct Label(String);

    impl KagariType for Label {
        fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
            let module = ModuleDecl::new(ModuleIdentity {
                package: PackageId("example".into()),
                path: vec!["typed".into()],
            });
            catalog
                .type_reference(&module.definition(DefinitionKind::Enum, "Label"))?
                .apply([])
        }
    }

    impl FromKagari for Label {
        fn from_kagari(
            cx: &mut ConversionContext<'_>,
            _: &TypeArgument,
            value: &Value,
        ) -> NativeResult<Self> {
            let Value::Enum(id) = value else {
                return Err(RuntimeError::module_validation("Label receiver"));
            };
            let payload = cx
                .runtime()
                .gc()
                .enum_snapshot(*id)
                .ok_or_else(|| RuntimeError::module_validation("Label payload"))?;
            let ty = cx.type_for::<String>()?;
            cx.decode_value(&ty, &payload.fields[0]).map(Self)
        }
    }
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("example::typed", &catalog);
    let mut label = module.define_enum("Label");
    label
        .variant("Text", [String::kagari_type(&catalog).unwrap()])
        .unwrap();
    let label = label.finish().unwrap();
    let catalog = module.declarations().clone();
    let binding = NativeBinding::typed_method(
        &catalog,
        |cx: &mut NativeContext<'_>,
         receiver: Label,
         (suffix,): ((String, String),)|
         -> NativeResult<String> {
            cx.collect_garbage()?;
            Ok(format!("{}{}{}", receiver.0, suffix.0, suffix.1))
        },
    )
    .unwrap();
    module
        .implement(label, |group| {
            group.inherent_impl(|methods| {
                let method = methods.define_method(
                    MethodDecl::instance("append")
                        .parameter("suffix", <(String, String)>::kagari_type(&catalog)?)
                        .returns(String::kagari_type(&catalog)?),
                )?;
                methods.bind_with(method, binding)
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        use example::typed::Label;
        fn main() -> String { Label::Text("雪").append(("🌱", "!")) }
    "#,
        &module,
    );
    assert!(
        script_equal(
            vm.runtime().gc(),
            &(vm.execute(&loaded, "main")
                .unwrap()
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result")),
            &(vm.runtime().gc().alloc_string("雪🌱!".into()).unwrap())
        )
        .unwrap()
    );
}

fn source_free_vm(source: &str, module: &NativeModule) -> (Vm, LoadedModule) {
    let program = compile_program(source, Some(module));
    let bytes = KbcArtifact::from_program(program, Default::default())
        .unwrap()
        .to_bytes()
        .unwrap();
    let program = KbcArtifact::from_bytes(&bytes).unwrap().program;
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
        },
        ..Default::default()
    });
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = runtime.load_program("typed-native", program).unwrap();
    (Vm::new(runtime), loaded)
}

#[test]
fn owned_native_arguments_and_business_results_survive_gc_and_source_free_loading() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("example::typed", &catalog);
    module
        .add_function(
            FunctionSpec::new("wrap").parameter_names(["input"]),
            |cx: &mut NativeContext<'_>,
             (input,): (Vec<String>,)|
             -> NativeResult<Result<Option<Vec<String>>, String>> {
                cx.collect_garbage()?;
                if input.is_empty() {
                    Ok(Err("empty input".into()))
                } else {
                    Ok(Ok(Some(cx.collect(input)?)))
                }
            },
        )
        .unwrap();
    module
        .add_function(
            FunctionSpec::new("inspect").parameter_names(["input"]),
            |cx: &mut NativeContext<'_>,
             (input,): (Result<Option<Vec<String>>, String>,)|
             -> NativeResult<i32> {
                cx.collect_garbage()?;
                Ok(match input {
                    Ok(Some(items)) if items == ["雪", "🌱"] => 40,
                    Err(message) if message == "empty input" => 2,
                    _ => -100,
                })
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        use example::typed::{wrap, inspect};
        fn main() -> i32 {
            val values = Vec::new(); values.push("雪"); values.push("🌱");
            inspect(wrap(values)) + inspect(wrap(Vec::new()))
        }
    "#,
        &module,
    );
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn typed_concrete_result_uses_the_compiler_selected_interface_adapter() {
    let standard = StandardDeclarations::default();
    let catalog = standard.catalog().unwrap();
    let list = standard
        .list()
        .apply([String::kagari_type(&catalog).unwrap()])
        .ty();
    let mut module = ModuleBuilder::new("example::typed", &catalog);
    module
        .add_function(
            FunctionSpec::new("split")
                .parameter_names(["text", "separator"])
                .returns(list),
            |cx: &mut NativeContext<'_>,
             (text, separator): (String, String)|
             -> NativeResult<Vec<String>> {
                cx.collect_garbage()?;
                cx.collect(text.split(&separator).map(str::to_owned))
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        use example::typed::split;
        fn main() -> usize { val fields = split("a:雪:", ":"); fields.len() }
    "#,
        &module,
    );
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::U64(3)
    );
}

#[test]
fn invalid_typed_registration_is_atomic_and_failed_output_preserves_callback_effects() {
    struct Invalid;

    impl KagariType for Invalid {
        fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
            i32::kagari_type(catalog)
        }
    }

    impl IntoKagari for Invalid {
        fn into_kagari(
            self,
            _: &mut ConversionContext<'_>,
            _: &TypeArgument,
        ) -> NativeResult<Value> {
            Ok(Value::Bool(true))
        }
    }
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("example::typed", &catalog);
    assert!(
        module
            .add_function(
                FunctionSpec::new("broken").returns(Type::bool()),
                |_: &mut NativeContext<'_>, (): ()| -> NativeResult<i32> {
                    panic!("mismatched callback")
                }
            )
            .is_err()
    );
    assert!(
        module
            .add_function(
                FunctionSpec::new("broken").parameter_names(["bad-name"]),
                |_: &mut NativeContext<'_>, (_value,): (i32,)| -> NativeResult<i32> {
                    panic!("invalid name")
                }
            )
            .is_err()
    );
    assert!(
        module
            .add_function(
                FunctionSpec::new("broken").parameter_names(["extra"]),
                |_: &mut NativeContext<'_>, (): ()| -> NativeResult<i32> {
                    panic!("mismatched callback")
                }
            )
            .is_err()
    );
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    module
        .add_function(
            FunctionSpec::new("broken"),
            move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<Invalid> {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Invalid)
            },
        )
        .unwrap();
    module
        .add_function(
            FunctionSpec::new("trap"),
            |_: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                Err(RuntimeError::resource_limit("explicit native failure"))
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        use example::typed::{broken, trap};
        fn main() -> i32 { broken() }
        fn fails() { trap(); }
        fn healthy() -> i32 { 42 }
    "#,
        &module,
    );
    assert!(vm.execute(&loaded, "main").is_err());
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    assert!(vm.execute(&loaded, "fails").is_err());
    assert_eq!(
        vm.execute(&loaded, "healthy")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn typed_host_entries_convert_composites_and_retain_returned_handles() {
    struct RetainedVec(RootedValue);

    impl KagariType for RetainedVec {
        fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
            Vec::<i32>::kagari_type(catalog)
        }
    }

    impl FromKagari for RetainedVec {
        const PRESERVES_IDENTITY: bool = true;
        fn from_kagari(
            cx: &mut ConversionContext<'_>,
            _: &TypeArgument,
            value: &Value,
        ) -> NativeResult<Self> {
            cx.runtime()
                .root_value(*value)
                .map(Self)
                .ok_or_else(|| RuntimeError::module_validation("retained array result"))
        }
    }
    let module = ModuleBuilder::new(
        "example::typed",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .finish()
    .unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        fn echo(value: Result<Option<Vec<(String, i32)>>, String>) -> Result<Option<Vec<(String, i32)>>, String> { value }
        fn retain(value: Vec<i32>) -> Vec<i32> { value }
        fn tuple(value: (i32, i32)) -> (i32, i32) { value }
        fn unit() {}
    "#,
        &module,
    );
    type Data = Result<Option<Vec<(String, i32)>>, String>;
    for value in [
        Ok(Some(vec![("雪".into(), 42)])),
        Ok(None),
        Err("business failure".into()),
    ] {
        let result: Data = vm.execute_typed(&loaded, "echo", (value.clone(),)).unwrap();
        assert_eq!(result, value);
    }
    let tuple: (i32, i32) = vm.execute_typed(&loaded, "tuple", ((20, 22),)).unwrap();
    assert_eq!(tuple, (20, 22));
    vm.execute_typed::<_, ()>(&loaded, "unit", ()).unwrap();
    let retained: RetainedVec = vm
        .execute_typed(&loaded, "retain", (vec![20, 22],))
        .unwrap();
    vm.runtime().collect_garbage().unwrap();
    let Value::Array(id) = retained.0.value(vm.runtime().gc()).unwrap() else {
        panic!("array")
    };
    assert_eq!(vm.runtime().gc().array_get(id, 1), Some(Value::I32(22)));
    drop(retained);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn typed_entry_signature_rejection_precedes_effects_and_data_failure_preserves_them() {
    struct Fails;

    impl KagariType for Fails {
        fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
            i32::kagari_type(catalog)
        }
    }

    impl FromKagari for Fails {
        fn from_kagari(
            _: &mut ConversionContext<'_>,
            _: &TypeArgument,
            _: &Value,
        ) -> NativeResult<Self> {
            Err(RuntimeError::module_validation(
                "data-dependent output rejection",
            ))
        }
    }
    let mut module = ModuleBuilder::new(
        "example::typed",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    module
        .add_function(
            FunctionSpec::new("effect"),
            move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        use example::typed::effect;
        fn run(value: i32) -> i32 { effect(); value }
    "#,
        &module,
    );
    assert!(
        vm.execute_typed::<_, String>(&loaded, "run", (42i32,))
            .is_err()
    );
    assert!(vm.execute_typed::<_, i32>(&loaded, "run", (true,)).is_err());
    assert!(vm.execute_typed::<_, i32>(&loaded, "run", ()).is_err());
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert!(
        vm.execute_typed::<_, Fails>(&loaded, "run", (42i32,))
            .is_err()
    );
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    assert_eq!(
        vm.execute_typed::<_, i32>(&loaded, "run", (42i32,))
            .unwrap(),
        42
    );
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn moving_a_result_out_of_its_report_keeps_it_rooted_without_manual_registration() {
    let module = ModuleBuilder::new(
        "example::typed",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .finish()
    .unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        fn make() -> Vec<i32> { val values = Vec::new(); values.push(42); values }
        fn idle() {}
    "#,
        &module,
    );
    let result = vm.execute(&loaded, "make").unwrap().return_value;
    let clone = result.clone();
    drop(result);
    vm.execute(&loaded, "idle").unwrap();
    vm.runtime().collect_garbage().unwrap();
    let Value::Array(id) = clone.value(vm.runtime().gc()).unwrap() else {
        panic!("array result")
    };
    assert_eq!(vm.runtime().gc().array_get(id, 0), Some(Value::I32(42)));
    let other = Runtime::default();
    assert!(clone.value(other.gc()).is_none());
    drop(clone);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn native_collect_polls_between_iterator_steps_and_releases_the_failed_call() {
    let cancellation = CancellationToken::default();
    let steps = Arc::new(AtomicUsize::new(0));
    let observed = steps.clone();
    let cancel = cancellation.clone();
    let mut module = ModuleBuilder::new(
        "example::typed",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    module
        .add_function(
            FunctionSpec::new("gather"),
            move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<Vec<i32>> {
                cx.collect((0..100).inspect(|_| {
                    if observed.fetch_add(1, Ordering::SeqCst) == 2 {
                        cancel.cancel();
                    }
                }))
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = source_free_vm(
        "use example::typed::gather; fn main() -> Vec<i32> { gather() } fn healthy() -> i32 { 42 }",
        &module,
    );
    let mut options = vm.runtime().execution_options();
    options.cancellation = cancellation;
    let session = vm.runtime().begin_execution(&loaded, options).unwrap();
    assert!(vm.execute(&loaded, "main").is_err());
    assert_eq!(steps.load(Ordering::SeqCst), 3);
    drop(session);
    assert_eq!(
        vm.execute_typed::<_, i32>(&loaded, "healthy", ()).unwrap(),
        42
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}
