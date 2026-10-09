use crate::compile_program;
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::ModuleSlot, module::BytecodeModuleSlot, program::BytecodeProgram,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    module::{LoadedModule, ModuleKey},
    native::module::NativeModule,
    value::Value,
};
use kagari_stdlib::modules;
use kagari_vm::vm::Vm;
use std::collections::HashSet;

fn program(answer: i32) -> BytecodeProgram {
    with_state(compile_program(
        &format!(
            "fn make() -> fn() -> i32 {{ val data = [{answer}]; || data[0] }} \
             fn answer() -> i32 {{ {answer} }}"
        ),
        None,
    ))
}

fn with_state(mut program: BytecodeProgram) -> BytecodeProgram {
    program.modules[program.root.index()]
        .module_slots
        .push(BytecodeModuleSlot {
            name: "saved".into(),
            ty: ValueType::HeapObject,
            mutable: true,
        });
    program
}

fn generic_program() -> BytecodeProgram {
    with_state(compile_program(
        "use std::cmp::Ordering; \
         trait Keep { fn keep<T: Ord>(self, a: T, b: T) -> fn() -> bool { || a.cmp(b) == Ordering::Less } } \
         impl Keep for i32 {} \
         fn make() -> fn() -> bool { val keeper: Keep = 0; keeper.keep(1, 2) }",
        None,
    ))
}

#[test]
fn generic_operation_environment_does_not_root_its_obsolete_program_cycle() {
    let mut vm = vm();
    let code = generic_program();
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
    vm.runtime()
        .write_module_slot(&old, ModuleSlot::new(0), closure)
        .unwrap();
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
    let code = generic_program();
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
    let current = vm.reload_program(&old, "environment", code).unwrap();
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
    let collected = vm.runtime().collect_garbage().unwrap();
    assert!(collected.reclaimed_operation_groups > 0);
    // Published code does not root an escaped invocation's operation records.
    vm.runtime().validate_loaded_module(&current).unwrap();
    let before = vm.runtime().resources().counters();
    let failure = vm
        .runtime()
        .make_closure(
            &snapshot.implementation,
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
fn old_module_closure_cycle_is_retained_only_by_external_roots() {
    let mut vm = vm();
    let old = vm.runtime_mut().load_program("cycle", program(42)).unwrap();
    let closure = vm
        .execute(&old, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    vm.runtime()
        .write_module_slot(&old, ModuleSlot::new(0), closure)
        .unwrap();
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
    vm.runtime()
        .write_module_slot(&left, ModuleSlot::new(0), from_right)
        .unwrap();
    vm.runtime()
        .write_module_slot(&right, ModuleSlot::new(0), from_left)
        .unwrap();
    let newer_left = vm.reload_program(&left, "left", code.clone()).unwrap();
    // The still-current right program reaches both old versions.
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    let newer_right = vm.reload_program(&right, "right", code).unwrap();
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
