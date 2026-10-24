mod applications;
mod interface_applications;
mod interface_handles;
mod interface_methods;
mod method_applications;
mod methods;
mod native_boxing;
mod primitives;
mod resource_termination;
mod script_values;
mod selected_methods;
mod shared_application;
use super::{compile_program, compile_without_foundation};
use crate::tests::native_boundary::support::round_trip_program;
use crate::{error::VmError, vm::Vm};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    gc::GcHeapConfig,
    module::Program,
    native::{
        binding::NativeResult, builder::ModuleBuilder, conversion::KagariType,
        declarations::FunctionDecl, function_handle::Function, module::NativeModule,
        objects::Object, typed::NativeContext, types::Type,
    },
    resource::RuntimeLimits,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

type ScalarCallback = Function<(i32,), i32>;

pub(super) fn fixture(source: &str, module: Option<&NativeModule>) -> (Vm, Program) {
    fixture_program(compile_program(source, module), module)
}

fn fixture_without_foundation(source: &str, module: Option<&NativeModule>) -> (Vm, Program) {
    fixture_program(compile_without_foundation(source, module), module)
}

fn fixture_program(
    program: DefinitionMetadata<BytecodeProgram>,
    module: Option<&NativeModule>,
) -> (Vm, Program) {
    let program = round_trip_program(program);
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        limits: RuntimeLimits {
            max_call_depth: Some(16),
        },
        ..Default::default()
    });
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    if let Some(module) = module {
        module.install(&mut runtime).unwrap();
    }
    let owner = runtime.load_program("functions", program).unwrap();
    (Vm::new(runtime), owner)
}

#[test]
fn bound_entries_check_visibility_signature_and_return_retained_objects() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut module = ModuleBuilder::new(
        "example::functions",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let observed = calls.clone();
    {
        let registered_function = module.define_function(FunctionDecl::new("tick")).unwrap();
        module
            .bind(
                registered_function.clone(),
                move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                    observed.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::functions::tick;
        pub struct Player { pub var hp: i32 }
        pub fn make() -> Player { tick(); Player { hp: 42 } }
        pub fn read(player: Player) -> i32 { player.hp }
        fn private() -> i32 { tick(); 1 }
    "#,
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    assert!(
        vm.runtime()
            .bind_function::<(), i32>(&_owner_program.root(), "private")
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_function::<(), i32>(&_owner_program.root(), "make")
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_function::<(i32,), Object>(&_owner_program.root(), "make")
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let make = vm
        .runtime()
        .bind_function::<(), Object>(&_owner_program.root(), "make")
        .unwrap();
    let read = vm
        .runtime()
        .bind_function::<(Object,), i32>(&_owner_program.root(), "read")
        .unwrap();
    let player = vm.call(&make, ()).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.call(&read, (player.clone(),)).unwrap(), 42);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let (foreign, _) = fixture("pub fn main() {}", None);

    assert!(foreign.call(&make, ()).is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop(player);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn typed_closures_reenter_native_and_retain_captures_until_last_handle() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let remembered: Arc<Mutex<Option<ScalarCallback>>> = Arc::default();
    let mut module = ModuleBuilder::new("example::functions", &catalog).unwrap();
    let saved = remembered.clone();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("apply")
                    .parameter(
                        "callback",
                        <Function<(i32,), i32>>::kagari_type(module.declarations()).unwrap(),
                    )
                    .parameter("value", Type::i32())
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                move |cx: &mut NativeContext<'_>,
                      (callback, value): (Function<(i32,), i32>, i32)|
                      -> NativeResult<i32> {
                    *saved.lock().unwrap() = Some(callback.clone());
                    cx.collect_garbage()?;
                    callback.call(cx, (value,))
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("increment")
                    .parameter("value", Type::i32())
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |cx: &mut NativeContext<'_>, (value,): (i32,)| -> NativeResult<i32> {
                    cx.collect_garbage()?;
                    Ok(value + 1)
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::functions::{apply, increment};
        pub fn main() -> i32 { val offset = [40]; apply(|x| increment(x + offset[0]), 1) }
        pub fn use_callback(callback: fn(i32) -> i32) -> i32 { callback(2) }
    "#,
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    let main = vm
        .runtime()
        .bind_function::<(), i32>(&_owner_program.root(), "main")
        .unwrap();
    assert_eq!(vm.call(&main, ()).unwrap(), 42);
    let callback = remembered.lock().unwrap().take().unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.call(&callback, (2,)).unwrap(), 43);
    let invoke = vm
        .runtime()
        .bind_function::<(Function<(i32,), i32>,), i32>(&_owner_program.root(), "use_callback")
        .unwrap();
    assert_eq!(vm.call(&invoke, (callback.clone(),)).unwrap(), 43);
    drop(callback);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn returned_closures_retain_bodies_while_bound_functions_select_current() {
    let source = |answer| {
        format!(
            r#"
            mod implementation {{
                pub fn value() -> i32 {{ {answer} }}
                pub fn closure() -> fn() -> i32 {{ val data = [{answer}]; || data[0] }}
            }}
            pub use crate::implementation::value as answer;
            pub use crate::implementation::closure as make;
            pub use crate as again;
            fn main() {{}}
            "#
        )
    };
    let (vm, old) = fixture_without_foundation(&source(41), None);
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    let answer = vm
        .runtime()
        .bind_function::<(), i32>(&_old_program.root(), "again::answer")
        .unwrap();
    assert_eq!(
        answer
            .prepare_call(vm.runtime())
            .unwrap()
            .owner()
            .bytecode
            .identity
            .path,
        ["implementation"]
    );
    for inaccessible in [
        "main",
        "value",
        "implementation::value",
        "::answer",
        "again",
    ] {
        assert!(
            vm.runtime()
                .bind_function::<(), i32>(&_old_program.root(), inaccessible)
                .is_err(),
            "{inaccessible}"
        );
    }
    let duplicate = vm
        .runtime()
        .bind_function::<(), i32>(&_old_program.root(), "answer")
        .unwrap();
    let make = vm
        .runtime()
        .bind_function::<(), Function<(), i32>>(&_old_program.root(), "make")
        .unwrap();
    let closure = vm.call(&make, ()).unwrap();
    let prepared = answer.prepare_call(vm.runtime()).unwrap();
    vm.reload_program(
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(&source(42), None),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.runtime().select_program(&_new_program).unwrap();
    let new_answer = vm
        .runtime()
        .bind_function::<(), i32>(&_new_program.root(), "answer")
        .unwrap();
    {
        let mut cx = vm.context(&new).unwrap();
        let _session = vm
            .runtime()
            .begin_execution(&new, vm.runtime().execution_options())
            .unwrap();
        assert_eq!(answer.call(&mut cx, ()).unwrap(), 42);
        assert_eq!(
            prepared
                .call_with_options::<_, i32>(&mut cx, (), vm.runtime().execution_options())
                .unwrap(),
            41
        );
        assert_eq!(closure.call(&mut cx, ()).unwrap(), 41);
        assert_eq!(new_answer.call(&mut cx, ()).unwrap(), 42);
    }
    drop(prepared);
    drop(answer);
    drop(make);
    assert!(
        !vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key()),
        "the retained closure keeps its exact body alive"
    );
    drop(closure);
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key()),
        "ordinary handles must not retain obsolete executable bodies"
    );
    assert_eq!(vm.call(&duplicate, ()).unwrap(), 42);
    drop(duplicate);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn public_script_function_handles_can_be_passed_back_as_callable_values() {
    let (vm, owner) = fixture(
        "pub fn double(x: i32) -> i32 { x * 2 } pub fn use_callback(callback: fn(i32) -> i32) -> i32 { callback(21) }",
        None,
    );
    let _owner_program = owner;
    let double = vm
        .runtime()
        .bind_function::<(i32,), i32>(&_owner_program.root(), "double")
        .unwrap();
    let invoke = vm
        .runtime()
        .bind_function::<(Function<(i32,), i32>,), i32>(&_owner_program.root(), "use_callback")
        .unwrap();
    assert_eq!(vm.call(&invoke, (double,)).unwrap(), 42);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn typed_reentry_traps_cancellation_and_depth_exhaustion_release_execution_state() {
    let mut module = ModuleBuilder::new(
        "example::functions",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    {
        let registered_function = module
            .define_function(
                FunctionDecl::new("apply")
                    .parameter(
                        "callback",
                        <Function<(i32,), i32>>::kagari_type(module.declarations()).unwrap(),
                    )
                    .parameter("value", Type::i32())
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |cx: &mut NativeContext<'_>,
                 (callback, value): (Function<(i32,), i32>, i32)|
                 -> NativeResult<i32> { callback.call(cx, (value,)) },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::functions::apply;
        pub fn divide(x: i32) -> i32 { 10 / x }
        pub fn recurse(x: i32) -> i32 { apply(|value| recurse(value), x) }
        pub fn healthy() -> i32 { 42 }
    "#,
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let divide = vm
        .runtime()
        .bind_function::<(i32,), i32>(&_owner_program.root(), "divide")
        .unwrap();
    let recurse = vm
        .runtime()
        .bind_function::<(i32,), i32>(&_owner_program.root(), "recurse")
        .unwrap();
    let healthy = vm
        .runtime()
        .bind_function::<(), i32>(&_owner_program.root(), "healthy")
        .unwrap();
    assert!(vm.call(&divide, (0,)).is_err());
    assert!(
        matches!(vm.call(&recurse, (1,)), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded)
    );
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert!(vm.runtime().execution_root().is_none());
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    let token = CancellationToken::default();
    let mut options = vm.runtime().execution_options();
    options.cancellation = token.clone();
    {
        let _session = vm.runtime().begin_execution(&owner, options).unwrap();
        token.cancel();
        assert!(
            matches!(vm.call(&healthy, ()), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::Cancelled)
        );
    }
    assert_eq!(vm.call(&healthy, ()).unwrap(), 42);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn retained_entry_cannot_escape_candidate_initialization_isolation() {
    let source = "pub fn answer() -> i32 { 42 }";
    let (vm, old) = fixture(source, None);
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    let old_function = vm
        .runtime()
        .bind_function::<(), i32>(&_old_program.root(), "answer")
        .unwrap();
    let candidate = vm
        .runtime()
        .stage_reload_program(
            &_old_program,
            Some("functions".into()),
            compile_program(source, None),
        )
        .unwrap();
    let new_function = vm
        .runtime()
        .bind_function::<(), i32>(&_old_program.root(), "answer")
        .unwrap();
    let prepared = new_function.prepare_call(vm.runtime()).unwrap();
    assert_eq!(prepared.owner().key(), old.key());
    assert_ne!(prepared.owner().key(), candidate.module().key());
    {
        let _session = vm
            .runtime()
            .begin_candidate_initialization(&candidate)
            .unwrap();
        assert!(
            matches!(vm.call(&old_function, ()), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert!(
            matches!(vm.call(&new_function, ()), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
    }
    assert_eq!(vm.call(&old_function, ()).unwrap(), 42);
}

#[test]
fn native_declaration_handles_bind_checked_installed_entries_directly() {
    let mut module = ModuleBuilder::new(
        "example::functions",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let add = {
        let registered_function = module
            .define_function(
                FunctionDecl::new("add")
                    .parameter("left", Type::i32())
                    .parameter("right", Type::i32())
                    .returns(Type::i32()),
            )
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                |_: &mut NativeContext<'_>, (left, right): (i32, i32)| -> NativeResult<i32> {
                    Ok(left + right)
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let (vm, owner) = fixture(
        "use example::functions::add; pub fn main() -> i32 { add(20, 22) }",
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    let function = vm
        .runtime()
        .bind_function_declaration::<(i32, i32), i32>(
            &_owner_program.root(),
            &add.definitions().resolve(add.id()).unwrap().to_path(),
        )
        .unwrap();
    assert_eq!(vm.call(&function, (20, 22)).unwrap(), 42);
}
