#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::{
    modules,
    tests::{
        execution::native_boundary::{compile_program, support::round_trip_program},
        support::{bytecode::reload_bytecode, invoke},
    },
};
use kagari_runtime::{
    Runtime,
    closure::ClosureRecord,
    module::ExecutableModule,
    native::{conversion::context::ConversionContext, module::NativeModule},
    value::carriers::RootedValue,
};

use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use std::{slice, sync::Arc};

fn program(answer: i32, marker: i32) -> DefinitionMetadata<BytecodeProgram> {
    let source = include_str!("../../../../../tests/fixtures/native/program-callback-cycle.kgr");
    assert_eq!(source.matches("42").count(), 1);
    assert_eq!(source.matches("100 + answer()").count(), 1);
    round_trip_program(compile_program(
        &source
            .replace("42", &answer.to_string())
            .replace("100 + answer()", &format!("{marker} + answer()")),
        None,
    ))
}

fn generic_program(cycle: bool) -> DefinitionMetadata<BytecodeProgram> {
    compile_program(
        if cycle {
            include_str!("../../../../../tests/fixtures/native/generic-callback-cycle.kgr")
        } else {
            include_str!("../../../../../tests/fixtures/native/generic-callback.kgr")
        },
        None,
    )
}

fn record(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    root: &RootedValue,
) -> Arc<ClosureRecord> {
    let mut cx = ConversionContext::new(runtime, owner).unwrap();
    let view = cx.import(root).unwrap();
    cx.runtime_mut().resolve_closure(&view).unwrap()
}

#[test]
fn generic_operation_environment_does_not_root_its_callback_cycle() {
    let mut vm = vm();
    let code = generic_program(true);
    let installed = load_bytecode(&mut vm, "generic-cycle", code.clone()).unwrap();
    let old = vm.select_program(&installed).unwrap();
    let root = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let snapshot = record(&mut vm, &old, &root);
    assert!(snapshot.environment.is_some());
    reload_bytecode(&mut vm, &installed, Some("generic-cycle".into()), code).unwrap();
    vm.collect_garbage().unwrap();
    let retained = vm.gc_stats();
    assert!(vm.modules().loaded(old.key()).is_some());
    assert_eq!(
        invoke(&mut vm, &old, "invoke", slice::from_ref(&root)).as_bool(),
        Some(true)
    );
    drop(root);
    vm.collect_garbage().unwrap();
    assert!(vm.gc_stats().objects < retained.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        retained.root_registrations - 1
    );
    assert_reclaimed(&mut vm, &old);
    // Immutable record/environment facts survive; they do not retain the graph.
    assert!(snapshot.environment.is_some());
}

#[test]
fn detached_closure_records_do_not_retain_released_executable_dependencies() {
    let mut vm = vm();
    let code = generic_program(false);
    let installed = load_bytecode(&mut vm, "environment", code.clone()).unwrap();
    let old = vm.select_program(&installed).unwrap();
    let root = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let old_snapshot = record(&mut vm, &old, &root);
    assert!(old_snapshot.environment.is_some());
    reload_bytecode(
        &mut vm,
        &installed,
        Some("environment".into()),
        code.clone(),
    )
    .unwrap();
    vm.collect_garbage().unwrap();
    assert!(
        vm.validate_executable_module(&old_snapshot.implementation)
            .is_ok()
    );
    drop(root);
    vm.collect_garbage().unwrap();
    assert_reclaimed(&mut vm, &old);
    let before = vm.resources().counters();
    assert!(
        vm.validate_executable_module(&old_snapshot.implementation)
            .is_err()
    );
    assert_eq!(vm.resources().counters(), before);
    assert!(!vm.is_quarantined());
    let current = vm.select_program(&installed).unwrap();
    let current_root = vm
        .execute_module(&current, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let snapshot = record(&mut vm, &current, &current_root);
    assert_eq!(
        invoke(&mut vm, &current, "invoke", slice::from_ref(&current_root)).as_bool(),
        Some(true)
    );
    drop(current_root);
    vm.collect_garbage().unwrap();
    // Current publication, rather than detached facts, still authorizes this body.
    vm.validate_executable_module(&snapshot.implementation)
        .unwrap();
    reload_bytecode(&mut vm, &installed, Some("environment".into()), code).unwrap();
    vm.collect_garbage().unwrap();
    assert_reclaimed(&mut vm, &current);
    let before = vm.resources().counters();
    assert!(
        vm.validate_executable_module(&snapshot.implementation)
            .is_err()
    );
    assert_eq!(vm.resources().counters(), before);
    vm.validate_executable_module(&vm.select_program(&installed).unwrap())
        .unwrap();
    assert!(!vm.is_quarantined());
}

fn vm() -> Runtime {
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules().unwrap(), &mut runtime).unwrap();
    runtime
}

fn assert_reclaimed(runtime: &mut Runtime, old: &ExecutableModule) {
    for member in old.members() {
        assert!(runtime.modules().loaded(member.key()).is_none());
        assert!(runtime.module_instance_snapshot(&member).is_none());
        assert!(runtime.validate_executable_module(&member).is_err());
    }
}

#[test]
fn old_callback_cycle_is_retained_only_by_external_roots() {
    let mut vm = vm();
    let installed = load_bytecode(&mut vm, "cycle", program(42, 100)).unwrap();
    let old = vm.select_program(&installed).unwrap();
    let root = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let snapshot = record(&mut vm, &old, &root);
    assert_eq!(snapshot.implementation.key(), old.key());
    reload_bytecode(&mut vm, &installed, Some("cycle".into()), program(99, 200)).unwrap();
    let current = vm.select_program(&installed).unwrap();
    vm.collect_garbage().unwrap();
    let retained = vm.gc_stats();
    vm.validate_executable_module(&snapshot.implementation)
        .unwrap();
    // Enter the exact old callback; its ordinary answer() call selects current code.
    assert_eq!(
        invoke(&mut vm, &current, "invoke", slice::from_ref(&root)).as_i32(),
        Some(199)
    );
    let fresh = vm
        .execute_module(&current, "make", vm.execution_options())
        .unwrap()
        .return_value;
    assert_eq!(
        invoke(&mut vm, &current, "invoke", slice::from_ref(&fresh)).as_i32(),
        Some(299)
    );
    drop(fresh);
    drop(root);
    vm.collect_garbage().unwrap();
    assert!(vm.gc_stats().objects < retained.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        retained.root_registrations - 1
    );
    assert_reclaimed(&mut vm, &old);
    assert!(vm.modules().loaded(current.key()).is_some());
}

fn pair(runtime: &mut Runtime, owner: &ExecutableModule) -> (RootedValue, RootedValue) {
    let result = runtime
        .execute_module(owner, "pair", runtime.execution_options())
        .unwrap()
        .return_value;
    let mut cx = ConversionContext::new(runtime, owner).unwrap();
    let tuple = cx.import(&result).unwrap();
    let callback = cx.tuple_element(&tuple, 0).unwrap();
    let array = cx.tuple_element(&tuple, 1).unwrap();
    (cx.retain(&callback).unwrap(), cx.retain(&array).unwrap())
}

#[test]
fn cycles_between_obsolete_programs_are_collected_together() {
    let mut vm = vm();
    let code = program(42, 100);
    let left_installed = load_bytecode(&mut vm, "left", code.clone()).unwrap();
    let left = vm.select_program(&left_installed).unwrap();
    let right_installed = load_bytecode(&mut vm, "right", code.clone()).unwrap();
    let right = vm.select_program(&right_installed).unwrap();
    let (from_left, left_array) = pair(&mut vm, &left);
    let (from_right, right_array) = pair(&mut vm, &right);
    drop(invoke(
        &mut vm,
        &left,
        "link",
        &[left_array, from_right.clone()],
    ));
    drop(invoke(&mut vm, &right, "link", &[right_array, from_left]));
    reload_bytecode(&mut vm, &left_installed, Some("left".into()), code.clone()).unwrap();
    let newer_left = vm.select_program(&left_installed).unwrap();
    vm.collect_garbage().unwrap();
    assert!(vm.modules().loaded(left.key()).is_some());
    assert!(vm.modules().loaded(right.key()).is_some());
    reload_bytecode(&mut vm, &right_installed, Some("right".into()), code).unwrap();
    let newer_right = vm.select_program(&right_installed).unwrap();
    vm.collect_garbage().unwrap();
    let retained = vm.gc_stats();
    assert!(vm.modules().loaded(left.key()).is_some());
    assert!(vm.modules().loaded(right.key()).is_some());
    drop(from_right);
    vm.collect_garbage().unwrap();
    assert!(vm.gc_stats().objects < retained.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        retained.root_registrations - 1
    );
    assert_reclaimed(&mut vm, &left);
    assert_reclaimed(&mut vm, &right);
    for current in [newer_left, newer_right] {
        assert!(vm.modules().loaded(current.key()).is_some());
    }
}
