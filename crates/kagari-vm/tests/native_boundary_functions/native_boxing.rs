use super::fixture;
use crate::{compile_program, native_boundary_interfaces::generic_identity_module};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{artifact::KbcArtifact, instruction::ModuleSlot, module::BytecodeModuleSlot};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        conversion::context::ConversionContext,
        declarations::FunctionDecl,
        function_handle::PinnedFunction,
        module::NativeModule,
        objects::Object,
        registration::FunctionSpec,
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_vm::vm::{Vm, owned::DriveResult};
use std::{
    num::NonZeroUsize,
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

type Add = PinnedFunction<(i32, i32), i32>;
type Callback = PinnedFunction<(), i32>;

#[test]
fn registered_native_entries_box_round_trip_and_run_inside_script_closures() {
    let calls = Arc::new(AtomicUsize::new(0));
    let text_calls = Arc::new(AtomicUsize::new(0));
    let mut module = ModuleBuilder::new(
        "example::boxed",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let observed = calls.clone();
    let add = module
        .add_function(
            FunctionSpec::new("add").parameter_names(["left", "right"]),
            move |cx: &mut NativeContext<'_>, (left, right): (i32, i32)| -> NativeResult<i32> {
                cx.collect_garbage()?;
                observed.fetch_add(1, Ordering::Relaxed);
                if left < 0 {
                    return Err(RuntimeError::module_validation("requested failure"));
                }
                Ok(left + right)
            },
        )
        .unwrap();
    let observed = text_calls.clone();
    let text = module
        .add_function(
            FunctionSpec::new("text"),
            move |_cx: &mut NativeContext<'_>, (): ()| -> NativeResult<String> {
                observed.fetch_add(1, Ordering::Relaxed);
                Ok("retained native result".to_owned())
            },
        )
        .unwrap();
    let through = module
        .define_function(
            FunctionDecl::new("through")
                .parameter("f", Type::function([Type::i32(), Type::i32()], Type::i32()))
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind_with(
            through,
            NativeBinding::new(
                vec![Codec::Value],
                Codec::Scalar(Type::i32().abi().clone()),
                |cx| {
                    let callback = cx.callable(0)?;
                    callback.call_values(cx, &[Value::I32(20), Value::I32(22)])
                },
            ),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let source = r#"
        use example::boxed::{add, text, through};
        pub fn evidence() -> i32 { add(20, 22) }
        pub fn invoke(f: fn(i32, i32) -> i32, left: i32, right: i32) -> i32 { f(left, right) }
        pub fn echo(f: fn(i32, i32) -> i32) -> fn(i32, i32) -> i32 { f }
        pub fn capture(f: fn(i32, i32) -> i32) -> fn() -> i32 { || f(20, 22) }
        pub fn wrong(f: fn(i32) -> i32) -> i32 { f(42) }
        pub fn native(f: fn(i32, i32) -> i32) -> i32 { through(f) }
        pub fn text_evidence() -> String { text() }
        pub fn owned(callback: fn() -> String) -> String { callback() }
    "#;
    let (vm, old) = fixture(source, Some(&module));
    // Native completion can park before returning to its script caller. Its
    // frame must retain the result without repeating the callback on resume.
    let callback = vm
        .runtime()
        .bind_function_declaration::<(), String>(&old, text.id())
        .unwrap();
    let callback = ConversionContext::new(vm.runtime(), &old)
        .unwrap()
        .encode(callback)
        .unwrap();
    let owner = vm
        .start(
            &old,
            "owned",
            &[callback.value(vm.runtime().gc()).unwrap()],
            Default::default(),
        )
        .unwrap();
    drop(callback);
    let mut parked_native_return = false;
    let mut completed = false;
    for _ in 0..32 {
        vm.runtime().collect_garbage().unwrap();
        let before = text_calls.load(Ordering::Relaxed);
        match vm.drive(&owner, NonZeroUsize::new(1).unwrap()).unwrap() {
            DriveResult::Runnable => {
                parked_native_return |= before == 0 && text_calls.load(Ordering::Relaxed) == 1;
            }
            DriveResult::Waiting => panic!("synchronous native entry cannot wait"),
            DriveResult::Complete(result) => {
                let result = result.unwrap();
                let Some(Value::Str(id)) = result.value(vm.runtime().gc()) else {
                    panic!("native string result")
                };
                assert_eq!(
                    &*vm.runtime().gc().string(id).unwrap(),
                    "retained native result"
                );
                completed = true;
                break;
            }
        }
    }
    assert!(parked_native_return && completed);
    assert_eq!(text_calls.load(Ordering::Relaxed), 1);
    drop(owner);
    let function = vm
        .runtime()
        .bind_function_declaration::<(i32, i32), i32>(&old, add.id())
        .unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(&old, "invoke", (function.clone(), 20, 22))
            .unwrap(),
        42
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(
        vm.execute_typed::<_, i32>(&old, "wrong", (function.clone(),))
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let returned: Add = vm.execute_typed(&old, "echo", (function.clone(),)).unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(&old, "native", (function.clone(),))
            .unwrap(),
        42
    );
    assert_eq!(vm.call(&returned, (19, 23)).unwrap(), 42);
    let callback: Callback = vm
        .execute_typed(&old, "capture", (function.clone(),))
        .unwrap();
    drop(function);
    let current = vm
        .reload_program(&old, "functions", compile_program(source, Some(&module)))
        .unwrap();
    assert_eq!(vm.call(&callback, ()).unwrap(), 42);
    assert_eq!(
        vm.execute_typed::<_, i32>(&current, "invoke", (returned.clone(), 21, 21))
            .unwrap(),
        42
    );
    assert!(
        vm.execute_typed::<_, i32>(&current, "invoke", (returned.clone(), -1, 0))
            .is_err()
    );
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert!(vm.runtime().execution_root().is_none());
    let before = calls.load(Ordering::Relaxed);
    let cancellation = CancellationToken::default();
    let mut options = vm.runtime().execution_options();
    options.cancellation = cancellation.clone();
    {
        let _session = vm.runtime().begin_execution(&current, options).unwrap();
        cancellation.cancel();
        assert!(vm.call(&returned, (20, 22)).is_err());
    }
    assert_eq!(calls.load(Ordering::Relaxed), before);
    assert_eq!(vm.call(&returned, (20, 22)).unwrap(), 42);
    let (foreign, _) = fixture(source, Some(&module));
    assert!(foreign.call(&returned, (20, 22)).is_err());
    drop((returned, callback));
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
    assert!(collected.reclaimed_modules.contains(&old.key()));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn generic_native_function_values_retain_applied_nominal_scopes() {
    type Identity = PinnedFunction<(Object,), Object>;
    let module = generic_identity_module();
    let declaration = module
        .to_declaration()
        .unwrap()
        .definition(DefinitionKind::Function, "identity");
    let source = r#"
        use example::generic_helper::identity;
        pub struct Item { pub var value: i32 }
        pub fn item() -> Item { identity(Item { value: 42 }) }
        pub fn invoke(f: fn(Item) -> Item, item: Item) -> Item { f(item) }
        pub fn echo(f: fn(Item) -> Item) -> fn(Item) -> Item { f }
    "#;
    let (mut vm, old) = fixture(source, Some(&module));
    let item_type = vm.runtime().bind_type(&old, "Item", &[]).unwrap();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), Object>(
            &old,
            &declaration,
            slice::from_ref(item_type.type_argument()),
        )
        .unwrap();
    let item: Object = vm.execute_typed(&old, "item", ()).unwrap();
    let echoed: Identity = vm.execute_typed(&old, "echo", (identity,)).unwrap();
    let returned: Object = vm
        .execute_typed(&old, "invoke", (echoed.clone(), item.clone()))
        .unwrap();
    let value = vm.runtime().bind_field::<i32>(&item_type, "value").unwrap();
    returned
        .set(&mut vm.context(&old).unwrap(), &value, 43)
        .unwrap();
    assert_eq!(
        item.get(&mut vm.context(&old).unwrap(), &value).unwrap(),
        43
    );
    let new = vm
        .runtime_mut()
        .load_program(
            "independent",
            compile_program(&source.replace("i32", "i64"), Some(&module)),
        )
        .unwrap();
    let other: Object = vm.execute_typed(&new, "item", ()).unwrap();
    assert!(
        vm.execute_typed::<_, Object>(&new, "invoke", (echoed.clone(), other.clone()))
            .is_err()
    );
    assert!(vm.call(&echoed, (other,)).is_err());
    assert!(vm.call(&echoed, (item.clone(),)).is_ok());
    drop((echoed, item, returned));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn native_function_values_cannot_invent_a_missing_nested_application() {
    let module = generic_identity_module();
    let declaration = module
        .to_declaration()
        .unwrap()
        .definition(DefinitionKind::Function, "identity");
    let (vm, owner) = fixture(
        r#"
        use example::generic_helper::identity;
        trait Relay { fn relay<T>(self, value: T) -> T { identity(value) } }
        impl Relay for i32 {}
        pub fn evidence() -> i32 { val relay: Relay = 0; relay.relay(42) }
        pub fn invoke(f: fn(i32) -> i32) -> i32 { f(42) }
        pub fn echo(f: fn(i32) -> i32) -> fn(i32) -> i32 { f }
    "#,
        Some(&module),
    );
    let ty = ConversionContext::new(vm.runtime(), &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let function = vm
        .runtime()
        .bind_function_application_declaration::<(i32,), i32>(&owner, &declaration, &[ty]);
    // No closed witness exists for the nested native application in this open
    // relay body; boxing must not invent evidence to bind an unavailable entry.
    assert_eq!(
        function.unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
}

#[test]
fn a_boxed_native_entry_does_not_root_its_obsolete_module_state_cycle() {
    let mut module = ModuleBuilder::new(
        "example::boxed",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let add = module
        .add_function(
            FunctionSpec::new("add").parameter_names(["a", "b"]),
            |_: &mut NativeContext<'_>, (a, b): (i32, i32)| -> NativeResult<i32> { Ok(a + b) },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let mut program = compile_program(
        "use example::boxed::add; pub fn evidence() -> i32 { add(20, 22) }",
        Some(&module),
    );
    program.modules[program.root.index()]
        .module_slots
        .push(BytecodeModuleSlot {
            name: "saved".into(),
            ty: ValueType::HeapObject,
            mutable: true,
        });
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let program = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .program;
    let mut runtime = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
    let old = runtime.load_program("cycle", program.clone()).unwrap();
    let vm = Vm::new(runtime);
    let native = vm
        .runtime()
        .bind_function_declaration::<(i32, i32), i32>(&old, add.id())
        .unwrap();
    let root = ConversionContext::new(vm.runtime(), &old)
        .unwrap()
        .encode(native)
        .unwrap();
    let value = root.value(vm.runtime().gc()).unwrap();
    assert!(
        vm.runtime()
            .resolve_closure(&value)
            .unwrap()
            .script_function()
            .is_none()
    );
    vm.runtime()
        .write_module_slot(&old, ModuleSlot::new(0), value)
        .unwrap();
    vm.reload_program(&old, "cycle", program).unwrap();
    assert!(
        !vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    let callback: Add = ConversionContext::new(vm.runtime(), &old)
        .unwrap()
        .decode(&root)
        .unwrap();
    assert_eq!(vm.call(&callback, (20, 22)).unwrap(), 42);
    drop((callback, root));
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
    assert!(collected.reclaimed_modules.contains(&old.key()));
}
