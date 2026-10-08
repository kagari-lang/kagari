use crate::{
    debug::{DebugSession, SourceBreakpoint},
    tests::{
        common::{compile_test_bytecode, standard_runtime},
        native_fixtures,
    },
    vm::{JitExecutionStatus, Vm, native::PreparedNativeEntry},
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, Register},
};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    Runtime, RuntimeConfig, error::RuntimeErrorKind, gc::GcHeapConfig, resource::RuntimeLimits,
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
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
        "fn make() -> Vec<i32> { [42] } fn main() -> Vec<i32> { val kept = make(); val other = [1, 2]; kept }",
    );
    for encoded in [false, true] {
        for jit in [false, true] {
            let mut runtime = runtime();
            let program = module.clone();
            let program = if encoded {
                let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
                let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
                decoded.validate_for_loader(&Default::default()).unwrap();
                decoded.program
            } else {
                program
            };
            let loaded = runtime.load_program("gc.kgr", program).unwrap();
            let vm = Vm::new(runtime);
            let report = if jit {
                vm.execute_prepared(&loaded, "main", &native_fixtures::unsupported())
                    .unwrap()
            } else {
                vm.execute(&loaded, "main").unwrap()
            };
            let Value::Array(array) = report
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result")
            else {
                panic!("array result")
            };
            assert!(vm.runtime().gc().stats().collections > 0);
            assert_eq!(
                vm.runtime().gc().array_snapshot(array),
                Some(vec![Value::I32(42)])
            );
            assert_eq!(vm.runtime().gc().active_roots(), 1);
            let retained = report.return_value;
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(vm.runtime().gc().allocated_objects(), 1);
            assert_eq!(vm.runtime().gc().array_get(array, 0), Some(Value::I32(42)));
            drop(retained);
            assert_eq!(vm.runtime().collect_garbage().unwrap().reclaimed_objects, 1);
        }
    }
}

#[test]
fn closures_keep_captured_objects_and_cells_alive_across_collection() {
    let module = compile_test_bytecode(
        r#"
fn make_reader() -> fn() -> i32 {
    val values = [42];
    || values[0]
}
fn make_counter() -> fn() -> i32 {
    var count = 40;
    || { count = count + 1; count }
}
fn main() -> i32 {
    val reader = make_reader();
    val counter = make_counter();
    val garbage = [1, 2, 3];
    val first = counter();
    val second = counter();
    reader() + second - first
}
"#,
    );
    let mut runtime = runtime();
    let loaded = runtime.load_program("closure_gc.kgr", module).unwrap();
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").unwrap();
    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(43)
    );
    assert!(vm.runtime().gc().stats().collections > 0);
    drop(report);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn closure_handles_reject_other_runtimes_and_reclaimed_slots() {
    let module = compile_test_bytecode("fn make() -> fn() -> i32 { || 42 }");
    let foreign_runtime = runtime();
    let mut owner_runtime = runtime();
    let loaded = owner_runtime
        .load_program("closure_handles.kgr", module)
        .unwrap();
    let vm = Vm::new(owner_runtime);
    let value = vm
        .execute(&loaded, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let Value::Closure(_) = value else {
        panic!("closure result")
    };
    assert!(foreign_runtime.resolve_closure(&value).is_err());
    let rooted = vm.runtime().root_value(value.clone()).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().resolve_closure(&value).is_ok());
    drop(rooted);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().resolve_closure(&value).is_err());
}

#[test]
fn malformed_closure_function_is_rejected_before_execution() {
    let mut module = compile_test_bytecode("fn main() -> i32 { val call = || 42; call() }");
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
        runtime
            .load_program("malformed_closure.kgr", module)
            .is_err()
    );
}

#[test]
fn rooted_closure_retains_its_old_program_after_new_publish() {
    let old = compile_test_bytecode("fn make() -> fn() -> i32 { || 41 }");
    let new = compile_test_bytecode("fn make() -> fn() -> i32 { || 42 }");
    let mut runtime = runtime();
    let loaded = runtime.load_program("closure_epoch.kgr", old).unwrap();
    let mut vm = Vm::new(runtime);
    let closure = vm
        .execute(&loaded, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let rooted = vm.runtime().root_value(closure.clone()).unwrap();
    let replacement = vm
        .runtime_mut()
        .load_program("closure_epoch.kgr", new)
        .unwrap();
    drop(loaded);
    vm.runtime().collect_garbage().unwrap();
    let snapshot = vm
        .runtime()
        .resolve_closure(&rooted.value(vm.runtime().gc()).unwrap())
        .unwrap();
    vm.runtime()
        .validate_loaded_module(&snapshot.implementation)
        .unwrap();
    assert_ne!(snapshot.implementation.key(), replacement.key());
    drop(rooted);
}

#[test]
fn native_scalar_execution_visits_the_same_collection_safepoint() {
    let mut runtime = runtime();
    let module = compile_test_bytecode("fn main() -> i32 { 42 }");
    let loaded = runtime.load_program("gc.kgr", module).unwrap();
    let dead = runtime
        .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let native =
        PreparedNativeEntry::Native(native_fixtures::install_i32::<42>(&runtime, &loaded, false));
    let vm = Vm::new(runtime);
    let report = vm.execute_prepared(&loaded, "main", &native).unwrap();
    assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(vm.runtime().gc().stats().collections > 0);
    assert!(vm.runtime().gc().array_len(dead).is_none());
}

#[test]
fn traps_release_frame_roots_and_call_depth() {
    let module = compile_test_bytecode("fn main() -> i32 { val temporary = [1, 2]; 1 / 0 }");
    {
        let mut runtime = runtime();
        let loaded = runtime.load_program("gc.kgr", module.clone()).unwrap();
        let vm = Vm::new(runtime);
        assert!(vm.execute(&loaded, "main").is_err());
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().allocated_objects(), 0);
        assert_eq!(vm.runtime().resources().counters().current_heap_units, 0);
    }
}

#[test]
fn cloned_debug_bindings_keep_inspected_objects_alive_after_the_session_is_replaced() {
    let source = "fn main() -> i32 { val kept = [7]; kept.len(); 42 }";
    let module = compile_test_bytecode(source);
    let mut runtime = standard_runtime(RuntimeConfig {
        ..Default::default()
    });
    let loaded = runtime.load_program("gc.kgr", module).unwrap();
    let mut session = DebugSession::new(&runtime).unwrap();
    session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "gc.kgr",
            source.find("kept.len").unwrap(),
        ))
        .unwrap();
    let mut vm = Vm::new(runtime);
    vm.attach_debug_session(session).unwrap();
    vm.execute(&loaded, "main").unwrap();
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
    vm.attach_debug_session(DebugSession::new(vm.runtime()).unwrap())
        .unwrap();
    let Value::Array(array) = binding.value else {
        panic!("inspected array")
    };
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 1);
    assert_eq!(vm.runtime().gc().array_get(array, 0), Some(Value::I32(7)));
    drop(binding);
    assert_eq!(vm.runtime().collect_garbage().unwrap().reclaimed_objects, 1);
}

#[test]
fn growing_execution_windows_keep_outer_values_alive_and_release_them_on_return() {
    let program = compile_test_bytecode(
        r#"
fn descend(n: i32, kept: Vec<i32>) -> Vec<i32> {
    if n == 0 { kept } else {
        val inner = [n];
        val returned = descend(n - 1, inner);
        kept.push(returned[0]);
        kept
    }
}
fn main() -> i32 {
    val original = [42];
    val returned = descend(32, original);
    returned[0] + returned[1]
}
"#,
    );
    let mut runtime = runtime();
    let loaded = runtime.load_program("growing_windows", program).unwrap();
    let vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").unwrap();
    assert_eq!(
        report.return_value.value(vm.runtime().gc()),
        Some(Value::I32(74))
    );
    assert!(vm.runtime().gc().stats().collections > 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    drop(report);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.runtime().gc().allocated_objects(), 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn collection_during_an_instruction_cursor_quarantines_without_further_writes() {
    let program = compile_test_bytecode("fn main() -> i32 { 42 }");
    let mut runtime = runtime();
    let loaded = runtime.load_program("cursor_borrow", program).unwrap();
    let entry = loaded
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .id;
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push(&runtime, loaded.slot(), entry, &[], None)
        .unwrap();
    let mut cursor = stack.cursor(&runtime).unwrap();
    let error = runtime.collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(
        cursor
            .write_register(Register::new(0), Value::I32(99))
            .is_err()
    );
    assert!(cursor.read_register(Register::new(0)).is_err());
    assert!(cursor.execute_region(&mut None).is_err());
    drop(cursor);
    drop(stack);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}
