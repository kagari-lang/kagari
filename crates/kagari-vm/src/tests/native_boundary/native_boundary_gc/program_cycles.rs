use crate::tests::native_boundary::compile_program;
use crate::vm::Vm;
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    module::{LoadedModule, ModuleKey},
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
    let closure = vm
        .execute(&old, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    assert!(
        vm.runtime()
            .resolve_closure(&closure)
            .unwrap()
            .environment
            .is_some()
    );
    let root = vm.runtime().root_value(closure).unwrap();
    vm.reload_program(&old, "generic-cycle", code).unwrap();
    let retained = vm.runtime().collect_garbage().unwrap();
    assert!(retained.reclaimed_modules.is_empty());
    assert!(vm.runtime().modules().loaded(old.key()).is_some());
    drop(root);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
    assert!(!vm.runtime().gc().validate_value(&closure));
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
    let value = vm
        .execute(&old, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let old_snapshot = (*vm.runtime().resolve_closure(&value).unwrap()).clone();
    let current = vm
        .reload_program(&old, "environment", code.clone())
        .unwrap();
    let value = vm
        .execute(&current, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let current_root = vm.runtime().root_value(value).unwrap();
    let snapshot = (*vm.runtime().resolve_closure(&value).unwrap()).clone();
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    let before = vm.runtime().resources().counters();
    let failure = vm
        .runtime()
        .make_closure(
            &snapshot.implementation,
            snapshot.script_function().unwrap(),
            snapshot.captures.clone(),
            old_snapshot.environment.clone(),
        )
        .unwrap_err();
    assert_eq!(failure.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(!vm.runtime().is_quarantined());
    assert_eq!(vm.runtime().resources().counters(), before);
    assert!(
        vm.runtime()
            .make_closure(
                &snapshot.implementation,
                snapshot.script_function().unwrap(),
                snapshot.captures.clone(),
                snapshot.environment.clone(),
            )
            .is_ok()
    );
    drop(current_root);
    let unrooted = vm.runtime().collect_garbage().unwrap();
    assert_eq!(unrooted.live_objects, 0);
    // Published application descriptors retain reusable environments, not values.
    assert!(
        vm.runtime()
            .make_closure(
                &snapshot.implementation,
                snapshot.script_function().unwrap(),
                snapshot.captures.clone(),
                snapshot.environment.clone(),
            )
            .is_ok()
    );
    let replacement = vm.reload_program(&current, "environment", code).unwrap();
    let collected = vm.runtime().collect_garbage().unwrap();
    assert!(collected.reclaimed_environments > 0);
    assert!(collected.reclaimed_operation_groups > 0);
    assert!(collected.reclaimed_modules.contains(&current.key()));
    // Detached snapshots cannot republish the retired descriptor graph into new code.
    vm.runtime().validate_loaded_module(&replacement).unwrap();
    let before = vm.runtime().resources().counters();
    let failure = vm
        .runtime()
        .make_closure(
            &replacement,
            snapshot.script_function().unwrap(),
            snapshot.captures.clone(),
            snapshot.environment.clone(),
        )
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

fn assert_reclaimed(runtime: &Runtime, old: &LoadedModule, reclaimed: &HashSet<ModuleKey>) {
    for member in old.members() {
        assert!(reclaimed.contains(&member.key()));
        assert!(runtime.modules().loaded(member.key()).is_none());
        assert!(runtime.module_instance_snapshot(&member).is_none());
    }
}

#[test]
fn old_callback_cycle_is_retained_only_by_external_roots() {
    let mut vm = vm();
    let old = vm.runtime_mut().load_program("cycle", program(42)).unwrap();
    let closure = vm
        .execute(&old, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(closure).unwrap();
    let current = vm.reload_program(&old, "cycle", program(99)).unwrap();
    let first = vm.runtime().collect_garbage().unwrap();
    assert!(!first.reclaimed_modules.contains(&old.key()));
    assert_eq!(first.live_objects, 2);
    let snapshot = vm.runtime().resolve_closure(&closure).unwrap();
    assert_eq!(snapshot.implementation.key(), old.key());
    let implementation = snapshot.implementation.clone();
    drop(snapshot);
    assert_eq!(
        vm.execute(&implementation, "answer")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    drop(root);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_objects, 2);
    assert_eq!(collected.live_objects, 0);
    assert!(!vm.runtime().gc().validate_value(&closure));
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
    let right = vm
        .runtime_mut()
        .load_program("right", code.clone())
        .unwrap();
    let from_left = vm
        .execute(&left, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let from_right = vm
        .execute(&right, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let left_snapshot = vm.runtime().resolve_closure(&from_left).unwrap();
    let right_snapshot = vm.runtime().resolve_closure(&from_right).unwrap();
    let Value::Array(left_array) = left_snapshot.captures[0] else {
        panic!("callback array")
    };
    let Value::Array(right_array) = right_snapshot.captures[0] else {
        panic!("callback array")
    };
    drop((left_snapshot, right_snapshot));
    vm.runtime()
        .gc()
        .array_set(left_array, 0, from_right)
        .unwrap();
    vm.runtime()
        .gc()
        .array_set(right_array, 0, from_left)
        .unwrap();
    let root = vm.runtime().root_value(from_right).unwrap();
    let newer_left = vm.reload_program(&left, "left", code.clone()).unwrap();
    // The retained right callback reaches both old executable versions.
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    let newer_right = vm.reload_program(&right, "right", code).unwrap();
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
