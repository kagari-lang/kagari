mod dto;
mod host_adapter;
mod scoped_capabilities;
mod scoped_pairs;
mod scoped_struct;
use crate::tests::native_boundary::{
    compile_program, publish_unused_version, support::round_trip_program,
};
use crate::vm::Vm;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId},
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::TypeArgument,
    gc::{GcHeapConfig, roots::RootedValue},
    module::{InstalledProgram, entry::ProgramEntry},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, IntoKagari, KagariType,
            context::{ConversionContext, ConversionLimits},
        },
        declarations::FunctionDecl,
        declarations::MethodDecl,
        module::NativeModule,
        objects::Object,
        payload::Opaque,
        typed::NativeContext,
        types::Type,
    },
    value::Value,
    value_semantics::script_equal,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::declaration::module::ModuleDecl;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn conversion_limits_terminate_only_active_sessions_and_release_scratch_roots() {
    let module = ModuleBuilder::new(
        "example::typed",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap()
    .finish()
    .unwrap();
    let (vm, owner) = source_free_vm("pub fn main() {}", &module);
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let runtime = vm.runtime();
    let input = ConversionContext::new(runtime, &owner)
        .unwrap()
        .encode(("a".to_owned(), "b".to_owned()))
        .unwrap();
    for limits in [
        ConversionLimits {
            max_depth: 1,
            ..Default::default()
        },
        ConversionLimits {
            max_nodes: 2,
            ..Default::default()
        },
        ConversionLimits {
            max_string_bytes: 1,
            ..Default::default()
        },
    ] {
        for active in [false, true] {
            for decode in [false, true] {
                let session = active.then(|| {
                    runtime
                        .begin_execution(&owner, runtime.execution_options())
                        .unwrap()
                });
                let roots = runtime.gc().active_roots();
                let error = {
                    let mut cx = ConversionContext::with_limits(runtime, &owner, limits).unwrap();
                    let error = if decode {
                        cx.decode::<(String, String)>(&input).unwrap_err()
                    } else {
                        cx.encode(("a".to_owned(), "b".to_owned())).unwrap_err()
                    };
                    assert_eq!(runtime.gc().active_roots(), roots);
                    error
                };
                assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
                if active {
                    assert_eq!(runtime.resources().termination(), Some(error.clone()));
                    assert_eq!(runtime.resources().poll_execution().unwrap_err(), error);
                } else {
                    assert!(runtime.resources().termination().is_none());
                    runtime.resources().poll_execution().unwrap();
                }
                drop(session);
                assert!(runtime.resources().termination().is_none());
                let value = ConversionContext::new(runtime, &owner)
                    .unwrap()
                    .encode(7i32)
                    .unwrap();
                assert_eq!(value.value(runtime.gc()), Some(Value::I32(7)));
                drop(value);
                assert_eq!(runtime.gc().active_roots(), roots);
                assert_eq!(runtime.resources().counters().current_call_depth, 0);
            }
        }
    }
    drop(input);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn script_objects_cross_typed_entries_without_exposing_heap_ids() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let module = ModuleBuilder::new("example::typed", &catalog)
        .unwrap()
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
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
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
fn typed_instance_method_receives_its_receiver_separately_from_tuple_arguments() {
    struct Label(String);

    impl KagariType for Label {
        fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
            let module = ModuleDecl::new(ModuleIdentity {
                package: PackageId("example".into()),
                path: vec!["typed".into()],
            });
            catalog
                .type_reference(&DefinitionPath {
                    module: module.identity,
                    path: vec![DefinitionPathSegment {
                        kind: DefinitionKind::Enum,
                        name: "Label".into(),
                        occurrence: 0,
                    }],
                })?
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
    let mut module = ModuleBuilder::new("example::typed", &catalog).unwrap();
    let mut label = module.define_enum("Label").unwrap();
    label
        .variant("Text", [String::kagari_type(&catalog).unwrap()])
        .unwrap();
    let label = label.finish().unwrap();
    let catalog = module.declarations().clone();
    module
        .implement(label, |group| {
            group.inherent_impl(|methods| {
                let method = methods.define_method(
                    MethodDecl::instance("append")
                        .parameter("suffix", <(String, String)>::kagari_type(&catalog)?)
                        .returns(String::kagari_type(&catalog)?),
                )?;
                methods.bind(
                    method,
                    |cx: &mut NativeContext<'_>,
                     receiver: Label,
                     (suffix,): ((String, String),)|
                     -> NativeResult<String> {
                        cx.collect_garbage()?;
                        Ok(format!("{}{}{}", receiver.0, suffix.0, suffix.1))
                    },
                )
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
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert!(
        script_equal(
            vm.runtime().gc(),
            &(vm.execute(&loaded, ProgramEntry::Main)
                .unwrap()
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result")),
            &(vm.runtime().gc().alloc_string("雪🌱!".into()).unwrap())
        )
        .unwrap()
    );
}

fn source_free_vm(source: &str, module: &NativeModule) -> (Vm, InstalledProgram) {
    let program = compile_program(source, Some(module));
    let program = round_trip_program(program);
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
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
    let mut module = ModuleBuilder::new("example::typed", &catalog).unwrap();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("wrap")
                    .parameter(
                        "input",
                        <Option<(String, String)>>::kagari_type(module.declarations()).unwrap(),
                    )
                    .returns(
                        <Result<Option<(String, String)>, String>>::kagari_type(
                            module.declarations(),
                        )
                        .unwrap(),
                    ),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |cx: &mut NativeContext<'_>,
                 (input,): (Option<(String, String)>,)|
                 -> NativeResult<Result<Option<(String, String)>, String>> {
                    cx.collect_garbage()?;
                    if input.is_none() {
                        Ok(Err("empty input".into()))
                    } else {
                        Ok(Ok(input))
                    }
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("inspect")
                    .parameter(
                        "input",
                        <Result<Option<(String, String)>, String>>::kagari_type(
                            module.declarations(),
                        )
                        .unwrap(),
                    )
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |cx: &mut NativeContext<'_>,
                 (input,): (Result<Option<(String, String)>, String>,)|
                 -> NativeResult<i32> {
                    cx.collect_garbage()?;
                    Ok(match input {
                        Ok(Some(items)) if items == ("雪".to_owned(), "🌱".to_owned()) => 40,
                        Err(message) if message == "empty input" => 2,
                        _ => -100,
                    })
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let module = module.finish().unwrap();
    let (mut vm, loaded) = source_free_vm(
        r#"
        use example::typed::{wrap, inspect};
        fn main() -> i32 {
            val values = Some(("雪", "🌱"));
            inspect(wrap(values)) + inspect(wrap(None))
        }
    "#,
        &module,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    publish_unused_version(&mut vm, &_loaded_program);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn typed_concrete_result_uses_the_compiler_selected_interface_adapter() {
    kagari_runtime::native_data! { struct FieldCount { len: u64 } }
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("example::typed", &catalog).unwrap();
    let fields = module.register_type::<Opaque<FieldCount>>().unwrap();
    let mut counted = module.define_trait("Counted").unwrap();
    counted
        .define_method(MethodDecl::instance("len").returns(Type::u64()))
        .unwrap();
    let counted = counted.finish().unwrap();
    module
        .implement(fields.clone(), |group| {
            group.trait_impl(counted.apply([])?, |methods| {
                methods.bind(
                    "len",
                    NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::u64()), |cx| {
                        cx.with_payload::<FieldCount, _>(0, |fields| Ok(Value::U64(fields.len)))
                    }),
                )
            })
        })
        .unwrap();
    let split = module
        .define_function(
            FunctionDecl::new("split")
                .parameter("text", String::kagari_type(module.declarations()).unwrap())
                .parameter(
                    "separator",
                    String::kagari_type(module.declarations()).unwrap(),
                )
                .returns(counted.apply([]).unwrap().ty()),
        )
        .unwrap();
    module
        .function(&split, |function| function.produces(fields))
        .unwrap();
    module
        .bind(
            split,
            |cx: &mut NativeContext<'_>,
             (text, separator): (String, String)|
             -> NativeResult<Opaque<FieldCount>> {
                cx.collect_garbage()?;
                cx.create_opaque(FieldCount {
                    len: text.split(&separator).count() as u64,
                })
            },
        )
        .unwrap();
    let (vm, program) = source_free_vm(
        include_str!("../../../tests/fixtures/native/typed_concrete_payload.kgr"),
        &module.finish().unwrap(),
    );
    let owner = vm.runtime().select_program(&program).unwrap();
    assert_eq!(vm.execute_typed::<_, u64>(&owner, "main", ()).unwrap(), 3);
}

#[test]
fn failed_output_preserves_callback_effects_and_traps_release_conversion_roots() {
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
    let mut module = ModuleBuilder::new("example::typed", &catalog).unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("broken")
                    .returns(<Invalid>::kagari_type(module.declarations()).unwrap()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<Invalid> {
                    observed.fetch_add(1, Ordering::SeqCst);
                    Ok(Invalid)
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    {
        let registered_function = module.define_function(FunctionDecl::new("trap")).unwrap();
        module
            .bind(
                registered_function.clone(),
                |_: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                    Err(RuntimeError::resource_limit("explicit native failure"))
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        use example::typed::{broken, trap};
        fn main() -> i32 { broken() }
        pub fn fails() { trap(); }
        pub fn healthy() -> i32 { 42 }
    "#,
        &module,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert!(vm.execute(&loaded, ProgramEntry::Main).is_err());
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
    struct RetainedTuple(RootedValue);

    impl KagariType for RetainedTuple {
        fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
            <(i32, i32)>::kagari_type(catalog)
        }
    }

    impl FromKagari for RetainedTuple {
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
    .unwrap()
    .finish()
    .unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        pub fn echo(value: Result<Option<((String, i32),)>, String>) -> Result<Option<((String, i32),)>, String> { value }
        pub fn retain(value: (i32, i32)) -> (i32, i32) { value }
        pub fn tuple(value: (i32, i32)) -> (i32, i32) { value }
        pub fn unit() {}
    "#,
        &module,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    type Data = Result<Option<((String, i32),)>, String>;
    for value in [
        Ok(Some((("雪".into(), 42),))),
        Ok(None),
        Err("business failure".into()),
    ] {
        let result: Data = vm.execute_typed(&loaded, "echo", (value.clone(),)).unwrap();
        assert_eq!(result, value);
    }
    let tuple: (i32, i32) = vm.execute_typed(&loaded, "tuple", ((20, 22),)).unwrap();
    assert_eq!(tuple, (20, 22));
    vm.execute_typed::<_, ()>(&loaded, "unit", ()).unwrap();
    let retained: RetainedTuple = vm.execute_typed(&loaded, "retain", ((20, 22),)).unwrap();
    vm.runtime().collect_garbage().unwrap();
    let Value::Tuple(id) = retained.0.value(vm.runtime().gc()).unwrap() else {
        panic!("array")
    };
    assert_eq!(
        vm.runtime().gc().tuple(id).unwrap().get(1).copied(),
        Some(Value::I32(22))
    );
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
    )
    .unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    {
        let registered_function = module.define_function(FunctionDecl::new("effect")).unwrap();
        module
            .bind(
                registered_function.clone(),
                move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                    observed.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        use example::typed::effect;
        pub fn run(value: i32) -> i32 { effect(); value }
    "#,
        &module,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
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
    .unwrap()
    .finish()
    .unwrap();
    let (vm, loaded) = source_free_vm(
        r#"
        pub fn make() -> [i32] { [42] }
        pub fn idle() {}
    "#,
        &module,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
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
    )
    .unwrap();
    {
        let registered_function = module
            .define_function(FunctionDecl::new("gather").returns(Type::i32()))
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<i32> {
                    let values = cx.collect((0..100).inspect(|_| {
                        if observed.fetch_add(1, Ordering::SeqCst) == 2 {
                            cancel.cancel();
                        }
                    }))?;
                    Ok(values.into_iter().sum())
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let module = module.finish().unwrap();
    let (vm, loaded) = source_free_vm(
        "use example::typed::gather; fn main() -> i32 { gather() } pub fn healthy() -> i32 { 42 }",
        &module,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let mut options = vm.runtime().execution_options();
    options.cancellation = cancellation;
    let session = vm.runtime().begin_execution(&loaded, options).unwrap();
    assert!(vm.execute(&loaded, ProgramEntry::Main).is_err());
    assert_eq!(steps.load(Ordering::SeqCst), 3);
    drop(session);
    assert_eq!(
        vm.execute_typed::<_, i32>(&loaded, "healthy", ()).unwrap(),
        42
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}
