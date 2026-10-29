use crate::tests::native_boundary::compile_program;
use crate::vm::Vm;
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    module::{ExecutableModule, ModuleKey},
    native::module::NativeModule,
    value::Value,
};
use kagari_stdlib::modules;
use std::collections::HashSet;

fn program(answer: i32) -> DefinitionMetadata<BytecodeProgram> {
    compile_program(
        &format!(
            "pub fn make() -> fn() -> i32 {{ val callbacks: [fn() -> i32] = [|| 0]; val result = || {{ callbacks[0]; answer() }}; callbacks[0] = result; result }} \
             pub fn answer() -> i32 {{ {answer} }}"
        ),
        None,
    )
}

fn generic_program(cycle: bool) -> DefinitionMetadata<BytecodeProgram> {
    let body = if cycle {
        "a.cmp(b); val callbacks: [fn() -> bool] = [|| false]; val result = || { callbacks[0]; a.cmp(b) == Ordering::Less }; callbacks[0] = result; result"
    } else {
        "a.cmp(b); || a.cmp(b) == Ordering::Less"
    };
    compile_program(
        &format!(
            "use std::cmp::Ordering; \
             trait Keep {{ fn keep<T: Ord>(self, a: T, b: T) -> fn() -> bool {{ {body} }} }} \
             impl Keep for i32 {{}} \
             pub fn make() -> fn() -> bool {{ val keeper: Keep = 0; keeper.keep(1, 2) }}"
        ),
        None,
    )
}

#[test]
fn generic_operation_environment_does_not_root_its_callback_cycle() {
    let mut vm = vm();
    let code = generic_program(true);
    let old = vm
        .runtime_mut()
        .load_program("generic-cycle", code.clone())
        .unwrap();
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    let closure_root = vm.execute(&old, "make").unwrap().return_value;
    // SAFETY: closure_root protects this value through its uses and traced publication.
    let closure = unsafe { closure_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    assert!(
        unsafe { vm.runtime().resolve_closure_transport(&closure) }
            .unwrap()
            .environment
            .is_some()
    );
    let root = unsafe { vm.runtime().gc().root_value_transport(closure) }.unwrap();
    drop(closure_root);
    vm.reload_program(&_old_program, Some("generic-cycle".into()), code)
        .unwrap();
    let retained = vm.runtime().collect_garbage().unwrap();
    assert!(retained.reclaimed_modules.is_empty());
    assert!(vm.runtime().modules().loaded(old.key()).is_some());
    drop(root);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
    assert!(!unsafe { vm.runtime().gc().validate_value_transport(&closure) });
    assert_reclaimed(
        vm.runtime(),
        &old,
        &collected.reclaimed_modules.into_iter().collect(),
    );
}

#[test]
fn detached_environment_snapshots_cannot_republish_released_executable_dependencies() {
    let mut vm = vm();
    let code = generic_program(false);
    let old = vm
        .runtime_mut()
        .load_program("environment", code.clone())
        .unwrap();
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    let old_root = vm.execute(&old, "make").unwrap().return_value;
    // SAFETY: old_root protects this value through its uses and traced publication.
    let value =
        unsafe { old_root.value_transport(vm.runtime().gc()) }.expect("retained execution result");
    let old_snapshot =
        (*unsafe { vm.runtime().resolve_closure_transport(&value) }.unwrap()).clone();
    drop(old_root);
    vm.reload_program(&_old_program, Some("environment".into()), code.clone())
        .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    let source_root = vm.execute(&current, "make").unwrap().return_value;
    // SAFETY: The source remains retained until the independent data root is published.
    let value = unsafe { source_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    let current_root = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    drop(source_root);
    let snapshot = (*unsafe { vm.runtime().resolve_closure_transport(&value) }.unwrap()).clone();
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    let before = vm.runtime().resources().counters();
    let failure = unsafe {
        vm.runtime().make_closure_transport(
            &snapshot.implementation,
            snapshot.script_function().unwrap(),
            snapshot.captures.clone(),
            old_snapshot.environment.clone(),
        )
    }
    .unwrap_err();
    assert_eq!(failure.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(!vm.runtime().is_quarantined());
    assert_eq!(vm.runtime().resources().counters(), before);
    assert!(
        unsafe {
            vm.runtime().make_closure_transport(
                &snapshot.implementation,
                snapshot.script_function().unwrap(),
                snapshot.captures.clone(),
                snapshot.environment.clone(),
            )
        }
        .is_ok()
    );
    drop(current_root);
    let unrooted = vm.runtime().collect_garbage().unwrap();
    assert_eq!(unrooted.live_objects, 0);
    // Published application descriptors retain reusable environments, not values.
    assert!(
        unsafe {
            vm.runtime().make_closure_transport(
                &snapshot.implementation,
                snapshot.script_function().unwrap(),
                snapshot.captures.clone(),
                snapshot.environment.clone(),
            )
        }
        .is_ok()
    );
    vm.reload_program(&_current_program, Some("environment".into()), code)
        .unwrap();
    let _replacement_program = _current_program.clone();
    let replacement = vm.runtime().select_program(&_replacement_program).unwrap();
    let collected = vm.runtime().collect_garbage().unwrap();
    assert!(collected.reclaimed_environments > 0);
    assert!(collected.reclaimed_operation_groups > 0);
    assert!(collected.reclaimed_modules.contains(&current.key()));
    // Detached snapshots cannot republish the retired descriptor graph into new code.
    vm.runtime()
        .validate_executable_module(&replacement)
        .unwrap();
    let before = vm.runtime().resources().counters();
    let failure = unsafe {
        vm.runtime().make_closure_transport(
            &replacement,
            snapshot.script_function().unwrap(),
            snapshot.captures.clone(),
            snapshot.environment.clone(),
        )
    }
    .unwrap_err();
    assert_eq!(failure.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(vm.runtime().resources().counters(), before);
    assert!(!vm.runtime().is_quarantined());
}

fn vm() -> Vm {
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules().unwrap(), &mut runtime).unwrap();
    Vm::new(runtime)
}

fn assert_reclaimed(runtime: &Runtime, old: &ExecutableModule, reclaimed: &HashSet<ModuleKey>) {
    for member in old.members() {
        assert!(reclaimed.contains(&member.key()));
        assert!(runtime.modules().loaded(member.key()).is_none());
        assert!(unsafe { runtime.module_instance_snapshot_transport(&member) }.is_none());
    }
}

#[test]
fn old_callback_cycle_is_retained_only_by_external_roots() {
    let mut vm = vm();
    let old = vm.runtime_mut().load_program("cycle", program(42)).unwrap();
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    let source_root = vm.execute(&old, "make").unwrap().return_value;
    // SAFETY: The source remains retained until the independent data root is published.
    let closure = unsafe { source_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    let root = unsafe { vm.runtime().gc().root_value_transport(closure) }.unwrap();
    drop(source_root);
    vm.reload_program(&_old_program, Some("cycle".into()), program(99))
        .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.runtime().select_program(&_current_program).unwrap();
    let first = vm.runtime().collect_garbage().unwrap();
    assert!(!first.reclaimed_modules.contains(&old.key()));
    assert_eq!(first.live_objects, 2);
    let snapshot = unsafe { vm.runtime().resolve_closure_transport(&closure) }.unwrap();
    assert_eq!(snapshot.implementation.key(), old.key());
    let implementation = snapshot.implementation.clone();
    drop(snapshot);
    // Retained closure bodies do not pin ordinary named calls to old code.
    assert_eq!(
        vm.execute(&implementation, "answer")
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(99)
    );
    drop(root);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_objects, 2);
    assert_eq!(collected.live_objects, 0);
    assert!(!unsafe { vm.runtime().gc().validate_value_transport(&closure) });
    assert_reclaimed(
        vm.runtime(),
        &old,
        &collected.reclaimed_modules.into_iter().collect(),
    );
    assert!(vm.runtime().modules().loaded(current.key()).is_some());
}

#[test]
fn cycles_between_obsolete_programs_are_collected_together() {
    let mut vm = vm();
    let code = program(42);
    let left = vm.runtime_mut().load_program("left", code.clone()).unwrap();
    let _left_program = left;
    let left = vm.runtime().select_program(&_left_program).unwrap();
    let right = vm
        .runtime_mut()
        .load_program("right", code.clone())
        .unwrap();
    let _right_program = right;
    let right = vm.runtime().select_program(&_right_program).unwrap();
    let left_root = vm.execute(&left, "make").unwrap().return_value;
    // SAFETY: left_root protects this value through its uses and traced publication.
    let from_left =
        unsafe { left_root.value_transport(vm.runtime().gc()) }.expect("retained execution result");
    let right_root = vm.execute(&right, "make").unwrap().return_value;
    // SAFETY: right_root protects this value through its uses and traced publication.
    let from_right = unsafe { right_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    let left_snapshot = unsafe { vm.runtime().resolve_closure_transport(&from_left) }.unwrap();
    let right_snapshot = unsafe { vm.runtime().resolve_closure_transport(&from_right) }.unwrap();
    let Value::Array(left_array) = left_snapshot.captures[0] else {
        panic!("callback array")
    };
    let Value::Array(right_array) = right_snapshot.captures[0] else {
        panic!("callback array")
    };
    drop((left_snapshot, right_snapshot));
    unsafe {
        vm.runtime()
            .gc()
            .array_set_transport(left_array, 0, from_right)
    }
    .unwrap();
    unsafe {
        vm.runtime()
            .gc()
            .array_set_transport(right_array, 0, from_left)
    }
    .unwrap();
    let root = unsafe { vm.runtime().gc().root_value_transport(from_right) }.unwrap();
    drop((left_root, right_root));
    vm.reload_program(&_left_program, Some("left".into()), code.clone())
        .unwrap();
    let _newer_left_program = _left_program.clone();
    let newer_left = vm.runtime().select_program(&_newer_left_program).unwrap();
    // The retained right callback reaches both old executable versions.
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    vm.reload_program(&_right_program, Some("right".into()), code)
        .unwrap();
    let _newer_right_program = _right_program.clone();
    let newer_right = vm.runtime().select_program(&_newer_right_program).unwrap();
    drop(root);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_objects, 4);
    assert_eq!(collected.live_objects, 0);
    let reclaimed = collected.reclaimed_modules.into_iter().collect();
    assert_reclaimed(vm.runtime(), &left, &reclaimed);
    assert_reclaimed(vm.runtime(), &right, &reclaimed);
    for current in [newer_left, newer_right] {
        assert!(vm.runtime().modules().loaded(current.key()).is_some());
    }
}
