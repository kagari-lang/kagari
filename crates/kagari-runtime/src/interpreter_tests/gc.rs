use crate::interpreter_tests::common::round_trip_bytecode;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::bytecode::reload_bytecode;
use crate::{
    Runtime, RuntimeConfig, gc::GcHeapConfig, module::entry::ProgramEntry, resource::RuntimeLimits,
    value::Value,
};
use crate::{
    debug::{DebugSession, SourceBreakpoint},
    execution::{JitExecutionStatus, native::PreparedNativeEntry},
    interpreter_tests::{
        common::{compile_test_bytecode, standard_runtime},
        native_fixtures,
    },
};
use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_contract::ids::FunctionRef;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    })
}

#[test]
fn frame_roots_preserve_returned_objects_across_calls_and_collection_safepoints() {
    let module = compile_test_bytecode(
        "pub fn make() -> [i32] { [42] } fn main() -> [i32] { val kept = make(); val other = [1, 2]; kept }",
    );
    for encoded in [false, true] {
        for jit in [false, true] {
            let mut runtime = runtime();
            let program = module.clone();
            let program = if encoded {
                round_trip_bytecode(program)
            } else {
                program
            };
            let loaded = load_bytecode(&mut runtime, "gc.kgr", program).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let vm = runtime;
            let report = if jit {
                vm.execute_prepared_module(
                    &loaded,
                    ProgramEntry::Main,
                    &native_fixtures::unsupported(),
                    vm.execution_options(),
                )
                .unwrap()
            } else {
                vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
                    .unwrap()
            };
            let Value::Array(array) = report
                .return_value
                .value(vm.gc())
                .expect("retained execution result")
            else {
                panic!("array result")
            };
            assert!(vm.gc().stats().collections > 0);
            assert_eq!(vm.gc().array_snapshot(array), Some(vec![Value::I32(42)]));
            assert_eq!(vm.gc().active_roots(), 1);
            let retained = report.return_value;
            vm.collect_garbage().unwrap();
            assert_eq!(vm.gc().allocated_objects(), 1);
            assert_eq!(vm.gc().array_get(array, 0), Some(Value::I32(42)));
            drop(retained);
            assert_eq!(vm.collect_garbage().unwrap().reclaimed_objects, 1);
        }
    }
}

#[test]
fn closures_keep_captured_objects_and_cells_alive_across_collection() {
    let module = compile_test_bytecode(include_str!(
        "../../tests/fixtures/gc/captured-object-and-cell.kgr"
    ));
    let mut runtime = runtime();
    let loaded = load_bytecode(&mut runtime, "closure_gc.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    assert_eq!(
        report
            .return_value
            .value(vm.gc())
            .expect("retained execution result"),
        Value::I32(43)
    );
    assert!(vm.gc().stats().collections > 0);
    drop(report);
    assert_eq!(vm.gc().active_roots(), 0);
}

#[test]
fn closure_handles_reject_other_runtimes_and_reclaimed_slots() {
    let module = compile_test_bytecode("pub fn make() -> fn() -> i32 { || 42 }");
    let foreign_runtime = runtime();
    let mut owner_runtime = runtime();
    let loaded = load_bytecode(&mut owner_runtime, "closure_handles.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = owner_runtime.select_program(&_loaded_program).unwrap();
    let vm = owner_runtime;
    let value = vm
        .execute_module(&loaded, "make", vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let Value::Closure(_) = value else {
        panic!("closure result")
    };
    assert!(foreign_runtime.resolve_closure(&value).is_err());
    let rooted = vm.root_value(value).unwrap();
    vm.collect_garbage().unwrap();
    assert!(vm.resolve_closure(&value).is_ok());
    drop(rooted);
    vm.collect_garbage().unwrap();
    assert!(vm.resolve_closure(&value).is_err());
}

#[test]
fn malformed_closure_function_is_rejected_before_execution() {
    let (definitions, mut module) =
        compile_test_bytecode("fn main() -> i32 { val call = || 42; call() }").into_parts();
    let instruction = module.modules[module.root.index()]
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.instructions)
        .find(|instruction| matches!(instruction, BytecodeInstruction::MakeClosure { .. }))
        .expect("compiled closure");
    if let BytecodeInstruction::MakeClosure { function, .. } = instruction {
        *function = FunctionRef::new(999);
    }
    let mut runtime = runtime();
    assert!(
        load_bytecode(
            &mut runtime,
            "malformed_closure.kgr",
            DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap()
        )
        .is_err()
    );
}

#[test]
fn rooted_closure_retains_its_old_program_after_new_publish() {
    let old = compile_test_bytecode("pub fn make() -> fn() -> i32 { || 41 }");
    let new = compile_test_bytecode("pub fn make() -> fn() -> i32 { || 42 }");
    let mut runtime = runtime();
    let loaded = load_bytecode(&mut runtime, "closure_epoch.kgr", old).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let closure = vm
        .execute_module(&loaded, "make", vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let rooted = vm.root_value(closure).unwrap();
    reload_bytecode(&mut vm, &_loaded_program, None, new).unwrap();
    let replacement = vm.select_program(&_loaded_program).unwrap();
    drop(loaded);
    vm.collect_garbage().unwrap();
    let snapshot = vm.resolve_closure(&rooted.value(vm.gc()).unwrap()).unwrap();
    vm.validate_executable_module(&snapshot.implementation)
        .unwrap();
    assert_ne!(snapshot.implementation.key(), replacement.key());
    drop(rooted);
}

#[test]
fn native_scalar_execution_visits_the_same_collection_safepoint() {
    let mut runtime = runtime();
    let module = compile_test_bytecode("fn main() -> i32 { 42 }");
    let loaded = load_bytecode(&mut runtime, "gc.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let dead = runtime
        .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let native =
        PreparedNativeEntry::Native(native_fixtures::install_i32::<42>(&runtime, &loaded, false));
    let vm = runtime;
    let report = vm
        .execute_prepared_module(&loaded, ProgramEntry::Main, &native, vm.execution_options())
        .unwrap();
    assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    assert_eq!(
        report
            .return_value
            .value(vm.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(vm.gc().stats().collections > 0);
    assert!(vm.gc().array_len(dead).is_none());
}

#[test]
fn traps_release_frame_roots_and_call_depth() {
    let module = compile_test_bytecode("fn main() -> i32 { val temporary = [1, 2]; 1 / 0 }");
    {
        let mut runtime = runtime();
        let loaded = load_bytecode(&mut runtime, "gc.kgr", module.clone()).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let vm = runtime;
        assert!(
            vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
                .is_err()
        );
        assert_eq!(vm.gc().active_roots(), 0);
        assert_eq!(vm.resources().counters().current_call_depth, 0);
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc().allocated_objects(), 0);
        assert_eq!(vm.resources().counters().current_heap_units, 0);
    }
}

#[test]
fn cloned_debug_bindings_keep_inspected_objects_alive_after_the_session_is_replaced() {
    let source = "fn main() -> i32 { val kept = [7]; kept.len(); 42 }";
    let module = compile_test_bytecode(source);
    let mut runtime = standard_runtime(RuntimeConfig {
        ..Default::default()
    });
    let loaded = load_bytecode(&mut runtime, "gc.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut session = DebugSession::new(&runtime).unwrap();
    session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "gc.kgr",
            source.find("kept.len").unwrap(),
        ))
        .unwrap();
    let mut vm = runtime;
    vm.attach_debug_session(session).unwrap();
    vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    let binding = vm
        .debug_session()
        .unwrap()
        .pauses()
        .iter()
        .flat_map(|pause| &pause.frames)
        .flat_map(|frame| &frame.bindings)
        .find(|binding| binding.name == "kept")
        .unwrap()
        .clone();
    vm.attach_debug_session(DebugSession::new(&vm).unwrap())
        .unwrap();
    let Value::Array(array) = binding.value else {
        panic!("inspected array")
    };
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 1);
    assert_eq!(vm.gc().array_get(array, 0), Some(Value::I32(7)));
    drop(binding);
    assert_eq!(vm.collect_garbage().unwrap().reclaimed_objects, 1);
}

#[test]
fn growing_execution_windows_keep_outer_values_alive_and_release_them_on_return() {
    let program = compile_test_bytecode(include_str!(
        "../../tests/fixtures/gc/recursive-window-roots.kgr"
    ));
    let mut runtime = runtime();
    let loaded = load_bytecode(&mut runtime, "growing_windows", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    assert_eq!(report.return_value.value(vm.gc()), Some(Value::I32(74)));
    assert!(vm.gc().stats().collections > 0);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    drop(report);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc().allocated_objects(), 0);
    assert_eq!(vm.gc().active_roots(), 0);
}
