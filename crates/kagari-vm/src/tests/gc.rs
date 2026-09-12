use crate::{Vm, tests::common::compile_test_bytecode};
use kagari_ir::bytecode::{BytecodeProgram, ModuleRef};
use kagari_runtime::{
    CapabilitySet, LanguageProfile, ResourcePolicy, Runtime, RuntimeConfig, SecurityContext,
    gc::GcHeapConfig, value::Value,
};

fn runtime(max_steps: Option<u64>) -> Runtime {
    Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        resources: ResourcePolicy {
            max_instruction_steps: max_steps,
            ..Default::default()
        },
        security: SecurityContext {
            profile: LanguageProfile {
                allow_jit: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                jit: true,
                ..Default::default()
            },
        },
        ..Default::default()
    })
}

#[test]
fn frame_roots_preserve_returned_objects_across_calls_and_collection_safepoints() {
    let module = compile_test_bytecode(
        "fn make() -> [i32] { [42] } fn main() -> [i32] { val kept = make(); val other = [1, 2]; kept }",
    );
    for encoded in [false, true] {
        for jit in [false, true] {
            let mut runtime = runtime(None);
            let program = BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module.clone()],
            };
            let program = if encoded {
                let artifact =
                    kagari_ir::bytecode::KbcArtifact::from_program(program, Default::default());
                let decoded =
                    kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
                        .unwrap();
                decoded.validate_for_loader(&Default::default()).unwrap();
                decoded.program
            } else {
                program
            };
            let loaded = runtime.load_program("gc.kgr", program).unwrap();
            let mut vm = Vm::new(runtime);
            let report = if jit {
                vm.execute_with_backend(
                    &loaded,
                    "main",
                    &mut kagari_jit_cranelift::CraneliftBackend::for_host().unwrap(),
                )
                .unwrap()
            } else {
                vm.execute(&loaded, "main").unwrap()
            };
            let Value::Array(array) = report.return_value else {
                panic!("array result")
            };
            assert!(vm.runtime().gc().stats().collections > 0);
            assert_eq!(
                vm.runtime().gc().array_snapshot(array),
                Some(vec![Value::I32(42)])
            );
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            let retained = vm.runtime().root_value(Value::Array(array)).unwrap();
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(vm.runtime().gc().allocated_objects(), 1);
            assert_eq!(vm.runtime().gc().array_get(array, 0), Some(Value::I32(42)));
            drop(retained);
            assert_eq!(vm.runtime().collect_garbage().unwrap().reclaimed_objects, 1);
        }
    }
}

#[test]
fn native_scalar_execution_visits_the_same_collection_safepoint() {
    let mut runtime = runtime(None);
    let dead = runtime.alloc_array(vec![Value::I32(7)]).unwrap();
    let module = compile_test_bytecode("fn main() -> i32 { 42 }");
    let loaded = runtime
        .load_program(
            "gc.kgr",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module],
            },
        )
        .unwrap();
    let mut vm = Vm::new(runtime);
    let report = vm
        .execute_with_backend(
            &loaded,
            "main",
            &mut kagari_jit_cranelift::CraneliftBackend::for_host().unwrap(),
        )
        .unwrap();
    assert_eq!(
        report.jit.unwrap().status,
        crate::JitExecutionStatus::Native
    );
    assert_eq!(report.return_value, Value::I32(42));
    assert!(vm.runtime().gc().stats().collections > 0);
    assert!(vm.runtime().gc().array_len(dead).is_none());
}

#[test]
fn trap_and_budget_exhaustion_release_frame_roots_and_call_depth() {
    let module = compile_test_bytecode("fn main() -> i32 { val temporary = [1, 2]; 1 / 0 }");
    for max_steps in [None, Some(4)] {
        let mut runtime = runtime(max_steps);
        let loaded = runtime
            .load_program(
                "gc.kgr",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![module.clone()],
                },
            )
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert!(vm.execute(&loaded, "main").is_err());
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().allocated_objects(), 0);
        assert_eq!(vm.runtime().resources().counters().current_heap_units, 0);
    }
}

#[test]
fn module_state_is_a_collection_root_until_its_version_is_reclaimed() {
    let mut module = compile_test_bytecode("fn init() -> [i32] { [7] } fn main() -> i32 { 42 }");
    module.module_init = Some(
        module
            .functions
            .iter()
            .find(|function| function.name == "init")
            .unwrap()
            .id,
    );
    module
        .module_slots
        .push(kagari_ir::bytecode::BytecodeModuleSlot {
            name: "state".into(),
            ty: kagari_ir::module::ValueType::HeapObject,
            mutable: true,
        });
    let mut runtime = runtime(None);
    let program = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![module],
    };
    let old = runtime.load_program("gc.kgr", program.clone()).unwrap();
    let mut vm = Vm::new(runtime);
    vm.execute(&old, "main").unwrap();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 1);
    {
        let mut instance = vm.runtime().module_instance_mut(&old).unwrap();
        instance.module_slots[0] = instance.init_result.take().unwrap();
    }
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 1);
    let new = vm
        .runtime_mut()
        .reload_program(&old, "gc.kgr", program)
        .unwrap();
    vm.execute(&new, "main").unwrap();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 2);
    assert_eq!(vm.runtime().modules().collect_unreachable_epochs().len(), 1);
    assert_eq!(vm.runtime().collect_garbage().unwrap().reclaimed_objects, 1);
}

#[test]
fn cloned_debug_bindings_keep_inspected_objects_alive_after_the_session_is_replaced() {
    let source = "fn main() -> i32 { val kept = [7]; kept.len(); 42 }";
    let module = compile_test_bytecode(source);
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                debug_attach: true,
                debug_breakpoints: true,
                debug_pause: true,
                debug_stack_inspection: true,
                debug_value_inspection: true,
                ..Default::default()
            },
        },
        debug_visibility: kagari_runtime::DebugVisibilityPolicy {
            allow_all_modules: true,
            ..Default::default()
        },
        ..Default::default()
    });
    let loaded = runtime
        .load_program(
            "gc.kgr",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module],
            },
        )
        .unwrap();
    let mut session = crate::DebugSession::new(&runtime).unwrap();
    session
        .add_breakpoint(crate::SourceBreakpoint::at_source_offset(
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
    vm.attach_debug_session(crate::DebugSession::new(vm.runtime()).unwrap())
        .unwrap();
    let Value::Array(array) = binding.value else {
        panic!("inspected array")
    };
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 1);
    assert_eq!(vm.runtime().gc().array_get(array, 0), Some(Value::I32(7)));
    drop(binding);
    assert_eq!(vm.runtime().collect_garbage().unwrap().reclaimed_objects, 1);
}
