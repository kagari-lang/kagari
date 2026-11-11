use crate::Runtime;
use crate::interpreter_tests::native_boundary::compile_program;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::bytecode::reload_bytecode;
use crate::{
    error::RuntimeErrorKind,
    module::{ExecutableModule, ModuleKey},
    native::module::NativeModule,
    value::Value,
};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
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
    let old = load_bytecode(&mut vm, "generic-cycle", code.clone()).unwrap();
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let closure = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    assert!(vm.resolve_closure(&closure).unwrap().environment.is_some());
    let root = vm.root_value(closure).unwrap();
    reload_bytecode(&mut vm, &_old_program, Some("generic-cycle".into()), code).unwrap();
    let retained = vm.collect_garbage().unwrap();
    assert!(retained.reclaimed_modules.is_empty());
    assert!(vm.modules().loaded(old.key()).is_some());
    drop(root);
    let collected = vm.collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
    assert!(!vm.gc().validate_value(&closure));
    assert_reclaimed(
        &vm,
        &old,
        &collected.reclaimed_modules.into_iter().collect(),
    );
}

#[test]
fn detached_environment_snapshots_cannot_republish_released_executable_dependencies() {
    let mut vm = vm();
    let code = generic_program(false);
    let old = load_bytecode(&mut vm, "environment", code.clone()).unwrap();
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let value = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let old_snapshot = (*vm.resolve_closure(&value).unwrap()).clone();
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("environment".into()),
        code.clone(),
    )
    .unwrap();
    let _current_program = _old_program.clone();
    let current = vm.select_program(&_current_program).unwrap();
    let value = vm
        .execute_module(&current, "make", vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let current_root = vm.root_value(value).unwrap();
    let snapshot = (*vm.resolve_closure(&value).unwrap()).clone();
    assert!(
        vm.collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    let before = vm.resources().counters();
    let failure = vm
        .make_closure(
            &snapshot.implementation,
            snapshot.script_function().unwrap(),
            snapshot.captures.clone(),
            old_snapshot.environment.clone(),
        )
        .unwrap_err();
    assert_eq!(failure.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(!vm.is_quarantined());
    assert_eq!(vm.resources().counters(), before);
    assert!(
        vm.make_closure(
            &snapshot.implementation,
            snapshot.script_function().unwrap(),
            snapshot.captures.clone(),
            snapshot.environment.clone(),
        )
        .is_ok()
    );
    drop(current_root);
    let unrooted = vm.collect_garbage().unwrap();
    assert_eq!(unrooted.live_objects, 0);
    // Published application descriptors retain reusable environments, not values.
    assert!(
        vm.make_closure(
            &snapshot.implementation,
            snapshot.script_function().unwrap(),
            snapshot.captures.clone(),
            snapshot.environment.clone(),
        )
        .is_ok()
    );
    reload_bytecode(&mut vm, &_current_program, Some("environment".into()), code).unwrap();
    let _replacement_program = _current_program.clone();
    let replacement = vm.select_program(&_replacement_program).unwrap();
    let collected = vm.collect_garbage().unwrap();
    assert!(collected.reclaimed_environments > 0);
    assert!(collected.reclaimed_operation_groups > 0);
    assert!(collected.reclaimed_modules.contains(&current.key()));
    // Detached snapshots cannot republish the retired descriptor graph into new code.
    vm.validate_executable_module(&replacement).unwrap();
    let before = vm.resources().counters();
    let failure = vm
        .make_closure(
            &replacement,
            snapshot.script_function().unwrap(),
            snapshot.captures.clone(),
            snapshot.environment.clone(),
        )
        .unwrap_err();
    assert_eq!(failure.kind(), RuntimeErrorKind::ModuleValidation);
    assert_eq!(vm.resources().counters(), before);
    assert!(!vm.is_quarantined());
}

fn vm() -> Runtime {
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules().unwrap(), &mut runtime).unwrap();
    runtime
}

fn assert_reclaimed(runtime: &Runtime, old: &ExecutableModule, reclaimed: &HashSet<ModuleKey>) {
    for member in old.members() {
        assert!(reclaimed.contains(&member.key()));
        assert!(runtime.modules().loaded(member.key()).is_none());
        assert!(runtime.module_instance_snapshot(&member).is_none());
    }
}

#[test]
fn old_callback_cycle_is_retained_only_by_external_roots() {
    let mut vm = vm();
    let old = load_bytecode(&mut vm, "cycle", program(42)).unwrap();
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let closure = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let root = vm.root_value(closure).unwrap();
    reload_bytecode(&mut vm, &_old_program, Some("cycle".into()), program(99)).unwrap();
    let _current_program = _old_program.clone();
    let current = vm.select_program(&_current_program).unwrap();
    let first = vm.collect_garbage().unwrap();
    assert!(!first.reclaimed_modules.contains(&old.key()));
    assert_eq!(first.live_objects, 2);
    let snapshot = vm.resolve_closure(&closure).unwrap();
    assert_eq!(snapshot.implementation.key(), old.key());
    let implementation = snapshot.implementation.clone();
    drop(snapshot);
    // Retained closure bodies do not pin ordinary named calls to old code.
    assert_eq!(
        vm.execute_module(&implementation, "answer", vm.execution_options())
            .unwrap()
            .return_value
            .value(vm.gc())
            .expect("retained execution result"),
        Value::I32(99)
    );
    drop(root);
    let collected = vm.collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_objects, 2);
    assert_eq!(collected.live_objects, 0);
    assert!(!vm.gc().validate_value(&closure));
    assert_reclaimed(
        &vm,
        &old,
        &collected.reclaimed_modules.into_iter().collect(),
    );
    assert!(vm.modules().loaded(current.key()).is_some());
}

#[test]
fn cycles_between_obsolete_programs_are_collected_together() {
    let mut vm = vm();
    let code = program(42);
    let left = load_bytecode(&mut vm, "left", code.clone()).unwrap();
    let _left_program = left;
    let left = vm.select_program(&_left_program).unwrap();
    let right = load_bytecode(&mut vm, "right", code.clone()).unwrap();
    let _right_program = right;
    let right = vm.select_program(&_right_program).unwrap();
    let from_left = vm
        .execute_module(&left, "make", vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let from_right = vm
        .execute_module(&right, "make", vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let left_snapshot = vm.resolve_closure(&from_left).unwrap();
    let right_snapshot = vm.resolve_closure(&from_right).unwrap();
    let Value::Array(left_array) = left_snapshot.captures[0] else {
        panic!("callback array")
    };
    let Value::Array(right_array) = right_snapshot.captures[0] else {
        panic!("callback array")
    };
    drop((left_snapshot, right_snapshot));
    vm.gc().array_set(left_array, 0, from_right).unwrap();
    vm.gc().array_set(right_array, 0, from_left).unwrap();
    let root = vm.root_value(from_right).unwrap();
    reload_bytecode(&mut vm, &_left_program, Some("left".into()), code.clone()).unwrap();
    let _newer_left_program = _left_program.clone();
    let newer_left = vm.select_program(&_newer_left_program).unwrap();
    // The retained right callback reaches both old executable versions.
    assert!(vm.collect_garbage().unwrap().reclaimed_modules.is_empty());
    reload_bytecode(&mut vm, &_right_program, Some("right".into()), code).unwrap();
    let _newer_right_program = _right_program.clone();
    let newer_right = vm.select_program(&_newer_right_program).unwrap();
    drop(root);
    let collected = vm.collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_objects, 4);
    assert_eq!(collected.live_objects, 0);
    let reclaimed = collected.reclaimed_modules.into_iter().collect();
    assert_reclaimed(&vm, &left, &reclaimed);
    assert_reclaimed(&vm, &right, &reclaimed);
    for current in [newer_left, newer_right] {
        assert!(vm.modules().loaded(current.key()).is_some());
    }
}
