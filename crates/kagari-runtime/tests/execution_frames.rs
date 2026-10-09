use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, Register},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    module::LoadedModule,
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[derive(Debug)]
struct ReentrantObserver;

impl ExecutionObserver for ReentrantObserver {
    fn observe(
        &mut self,
        runtime: &Runtime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        let module = runtime.execution_root().unwrap();
        let nested = runtime.enter_execution_stack(&module)?;
        nested.push(runtime, module.slot(), FunctionRef::new(0), &[], None)
    }
}

fn loaded(runtime: &mut Runtime) -> LoadedModule {
    loaded_with_instructions(runtime, vec![BytecodeInstruction::Return(None)])
}

fn loaded_with_instructions(
    runtime: &mut Runtime,
    instructions: Vec<BytecodeInstruction>,
) -> LoadedModule {
    let function = BytecodeFunction {
        id: FunctionRef::new(0),
        name: "main".into(),
        register_count: 1,
        metadata: FunctionMetadata {
            registers: vec![ValueType::HeapObject],
            roots: RootSlotLayout::from_types(&[], &[ValueType::HeapObject]),
            ..Default::default()
        },
        instructions,
        ..Default::default()
    };
    runtime
        .load_program(
            "frames",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule {
                    types: vec![ValueType::Unit, ValueType::HeapObject],
                    function_table: vec![FunctionRecord {
                        id: function.id,
                        identity: function.identity.clone(),
                        name: function.name.clone(),
                        params: vec![],
                        return_type: ValueType::Unit,
                        effects: Default::default(),
                    }],
                    functions: vec![function],
                    ..Default::default()
                }],
            },
        )
        .unwrap()
}

#[test]
fn nested_scopes_share_one_stack_and_unwind_only_their_own_roots() {
    let mut runtime = Runtime::default();
    let module = loaded(&mut runtime);
    let outer = runtime.enter_execution_stack(&module).unwrap();
    outer
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let first = Value::Array(
        runtime
            .alloc_array(&module, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
            .unwrap(),
    );
    outer
        .current_mut()
        .unwrap()
        .write_register(&runtime, Register::new(0), first)
        .unwrap();
    let nested = runtime.enter_execution_stack(&module).unwrap();
    nested
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let second = Value::Array(
        runtime
            .alloc_array(&module, Ty::Builtin(BuiltinType::I32), vec![Value::I32(2)])
            .unwrap(),
    );
    nested
        .current_mut()
        .unwrap()
        .write_register(&runtime, Register::new(0), second)
        .unwrap();
    assert_eq!(outer.frames().unwrap().len(), 2);
    assert_eq!(nested.frames().unwrap()[0].loaded().key(), module.key());
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&first));
    assert!(runtime.gc().validate_value(&second));
    drop(nested);
    assert_eq!(outer.frames().unwrap().len(), 1);
    assert_eq!(runtime.resources().counters().current_call_depth, 1);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&first));
    assert!(!runtime.gc().validate_value(&second));
    drop(outer);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert!(runtime.execution_root().is_none());
    runtime.collect_garbage().unwrap();
    assert!(!runtime.gc().validate_value(&first));
}

#[test]
fn root_depth_peaks_follow_actual_frames_and_reset_between_roots() {
    let mut runtime = Runtime::default();
    let module = loaded(&mut runtime);
    for depth in [2, 1] {
        let session = runtime
            .begin_execution(&module, runtime.execution_options())
            .unwrap();
        let stack = runtime.enter_execution_stack(&module).unwrap();
        for _ in 0..depth {
            stack
                .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
        }
        assert_eq!(session.counters().current_call_depth, depth);
        drop(stack);
        assert_eq!(session.counters().current_call_depth, 0);
        assert_eq!(session.counters().peak_call_depth, depth);
    }
    assert_eq!(runtime.resources().counters().peak_call_depth, 2);
}

#[test]
fn invalid_frame_access_quarantines_but_scope_cleanup_still_releases_every_root() {
    let mut runtime = Runtime::default();
    let module = loaded(&mut runtime);
    let stack = runtime.enter_execution_stack(&module).unwrap();
    stack
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    assert_eq!(
        stack
            .current()
            .unwrap()
            .read_register(&runtime, Register::new(9))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::EngineFault
    );
    assert!(runtime.is_quarantined());
    drop(stack);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
}

#[test]
fn out_of_order_stack_drop_is_isolated_without_double_cleanup() {
    let mut runtime = Runtime::default();
    let module = loaded(&mut runtime);
    let outer = runtime.enter_execution_stack(&module).unwrap();
    outer
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let nested = runtime.enter_execution_stack(&module).unwrap();
    nested
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    drop(outer);
    assert!(runtime.is_quarantined());
    assert!(nested.current().is_err());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    drop(nested);
    assert!(runtime.execution_root().is_none());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
}

#[test]
fn observers_cannot_reenter_a_borrowed_stack_or_replace_the_root_observer() {
    let mut runtime = Runtime::default();
    let module = loaded(&mut runtime);
    runtime.set_execution_observer(ReentrantObserver).unwrap();
    let stack = runtime.enter_execution_stack(&module).unwrap();
    assert!(runtime.attach_execution_observer().unwrap());
    assert!(!runtime.attach_execution_observer().unwrap());
    assert_eq!(
        runtime
            .set_execution_observer(ReentrantObserver)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    stack
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    assert_eq!(
        runtime
            .observe_execution(ExecutionEvent::BeforeInstruction)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::EngineFault
    );
    drop(stack);
    assert!(runtime.is_quarantined());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert!(runtime.execution_root().is_none());
}

#[test]
fn owned_observer_initialization_and_replacement_obey_session_and_borrow_scopes() {
    #[derive(Debug, Default)]
    struct Probe {
        begins: usize,
        observations: usize,
    }

    impl ExecutionObserver for Probe {
        fn begin(&mut self, _: &Runtime, _: &LoadedModule) -> Result<(), RuntimeError> {
            self.begins += 1;
            if self.begins == 1 {
                return Err(RuntimeError::module_validation("initialization rejected"));
            }
            Ok(())
        }

        fn observe(
            &mut self,
            _: &Runtime,
            _: ExecutionEvent,
            _: &[ExecutionFrame],
        ) -> Result<(), RuntimeError> {
            self.observations += 1;
            Ok(())
        }
    }

    let mut runtime = Runtime::default();
    let module = loaded(&mut runtime);
    runtime.set_execution_observer(Probe::default()).unwrap();
    let stack = runtime.enter_execution_stack(&module).unwrap();
    assert!(runtime.attach_execution_observer().is_err());
    runtime
        .observe_execution(ExecutionEvent::BeforeInstruction)
        .unwrap();
    assert_eq!(
        runtime.execution_observer::<Probe>().unwrap().observations,
        0
    );
    assert!(runtime.attach_execution_observer().unwrap());
    assert!(!runtime.attach_execution_observer().unwrap());
    assert_eq!(runtime.execution_observer::<Probe>().unwrap().begins, 2);
    assert!(runtime.clear_execution_observer().is_err());
    runtime
        .observe_execution(ExecutionEvent::BeforeInstruction)
        .unwrap();
    assert_eq!(
        runtime.execution_observer::<Probe>().unwrap().observations,
        1
    );
    drop(stack);
    let retained = runtime.execution_observer::<Probe>().unwrap();
    assert!(runtime.clear_execution_observer().is_err());
    assert!(runtime.set_execution_observer(Probe::default()).is_err());
    assert_eq!(retained.begins, 2);
    drop(retained);
    runtime.clear_execution_observer().unwrap();
    assert!(runtime.execution_observer::<Probe>().is_none());
    assert!(!runtime.is_quarantined());
}

#[test]
fn ending_a_suspended_session_does_not_count_candidate_frames_as_leaks() {
    let mut runtime = Runtime::default();
    let old = loaded(&mut runtime);
    let outer = runtime
        .begin_execution(&old, runtime.execution_options())
        .unwrap();
    let candidate = runtime
        .stage_reload_program(
            &old,
            "frames",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![old.to_unverified(&Default::default()).unwrap()],
            },
        )
        .unwrap();
    let old_object = Value::Array(
        runtime
            .alloc_array(&old, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
            .unwrap(),
    );
    let initialization = runtime.begin_candidate_initialization(&candidate).unwrap();
    let stack = runtime.enter_execution_stack(candidate.module()).unwrap();
    assert_eq!(
        stack
            .push(
                &runtime,
                candidate.module().slot(),
                FunctionRef::new(0),
                &[old_object],
                None
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    stack
        .push(
            &runtime,
            candidate.module().slot(),
            FunctionRef::new(0),
            &[],
            None,
        )
        .unwrap();
    assert_eq!(runtime.resources().counters().current_call_depth, 1);
    let inspected = stack.frames().unwrap();
    drop(outer);
    assert!(!runtime.is_quarantined());
    assert_eq!(inspected.len(), 1);
    assert_eq!(
        runtime.modules().retention_counts(old.key()).active_calls,
        0
    );
    drop(inspected);
    assert!(stack.current().is_ok());
    drop(stack);
    drop(initialization);
    assert!(runtime.execution_root().is_none());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    runtime.publish_staged_reload(candidate).unwrap();
}

#[test]
fn suspended_session_frames_cannot_be_used_during_candidate_initialization() {
    let mut runtime = Runtime::default();
    let old = loaded(&mut runtime);
    let outer = runtime.enter_execution_stack(&old).unwrap();
    outer
        .push(&runtime, old.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let candidate = runtime
        .stage_reload_program(
            &old,
            "frames",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![old.to_unverified(&Default::default()).unwrap()],
            },
        )
        .unwrap();
    let initialization = runtime.begin_candidate_initialization(&candidate).unwrap();
    assert_eq!(
        outer.current().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert!(runtime.is_quarantined());
    drop(initialization);
    drop(candidate);
    drop(outer);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.modules().loaded_count(), 1);
    assert!(runtime.execution_root().is_none());
}

#[test]
fn native_polling_preserves_cancellation_offsets_and_cleanup() {
    use kagari_abi::native_call::{JIT_STATUS_CANCELLED, JIT_STATUS_OK};
    use kagari_runtime::{RuntimeConfig, jit_abi::jit_poll_execution, resource::RuntimeLimits};
    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    });
    let module = loaded_with_instructions(
        &mut runtime,
        vec![
            BytecodeInstruction::Jump {
                target: kagari_bytecode::instruction::JumpTarget::new(1),
            },
            BytecodeInstruction::Jump {
                target: kagari_bytecode::instruction::JumpTarget::new(1),
            },
            BytecodeInstruction::Return(None),
        ],
    );
    let token = kagari_common::cancellation::CancellationToken::default();
    let mut options = runtime.execution_options();
    options.cancellation = token.clone();
    let _session = runtime.begin_execution(&module, options).unwrap();
    let stack = runtime.enter_execution_stack(&module).unwrap();
    stack
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    for offset in 0..2 {
        assert_eq!(
            unsafe { jit_poll_execution(&runtime, offset) },
            JIT_STATUS_OK
        );
    }
    token.cancel();
    assert_eq!(
        unsafe { jit_poll_execution(&runtime, 2) },
        JIT_STATUS_CANCELLED
    );

    assert_eq!(
        runtime.capture_error_trace().frames[0].instruction_offset,
        2
    );
    drop(stack);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn native_polling_requires_an_active_validated_program_point() {
    use kagari_abi::native_call::JIT_STATUS_ENGINE_FAULT;
    use kagari_runtime::jit_abi::jit_poll_execution;
    let runtime = Runtime::default();
    assert_eq!(
        unsafe { jit_poll_execution(&runtime, 0) },
        JIT_STATUS_ENGINE_FAULT
    );

    let mut runtime = Runtime::default();
    let module = loaded(&mut runtime);
    let stack = runtime.enter_execution_stack(&module).unwrap();
    stack
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    assert_eq!(
        unsafe { jit_poll_execution(&runtime, 1) },
        JIT_STATUS_ENGINE_FAULT
    );
}

#[test]
fn interpreter_frame_fetch_preserves_instruction_offsets() {
    let mut runtime = Runtime::default();
    let module = loaded_with_instructions(
        &mut runtime,
        vec![
            BytecodeInstruction::Jump {
                target: kagari_bytecode::instruction::JumpTarget::new(1),
            },
            BytecodeInstruction::Return(None),
        ],
    );
    let stack = runtime.enter_execution_stack(&module).unwrap();
    stack
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    for index in 0..2 {
        let instruction = stack.current_mut().unwrap().next_instruction().unwrap();
        assert_eq!(stack.current().unwrap().instruction_offset(), index);
        assert_eq!(
            matches!(instruction, BytecodeInstruction::Jump { .. }),
            index == 0
        );
        runtime.resources().poll_execution().unwrap();
    }

    assert!(stack.current_mut().unwrap().next_instruction().is_none());
}

#[test]
fn frame_access_rejects_a_foreign_context_before_changing_slots_or_program_point() {
    let mut runtime = Runtime::default();
    let module = loaded(&mut runtime);
    let foreign = Runtime::default();
    let stack = runtime.enter_execution_stack(&module).unwrap();
    assert_eq!(
        stack
            .push(&foreign, module.slot(), FunctionRef::new(0), &[], None)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(stack.is_empty().unwrap());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    stack
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let original = Value::Array(
        runtime
            .alloc_array(&module, Ty::Builtin(BuiltinType::I32), vec![Value::I32(3)])
            .unwrap(),
    );
    let mut frame = stack.current_mut().unwrap();
    frame
        .write_register(&runtime, Register::new(0), original)
        .unwrap();
    for error in [
        frame.read_register(&foreign, Register::new(0)).unwrap_err(),
        frame
            .write_register(&foreign, Register::new(0), Value::Unit)
            .unwrap_err(),
        frame.jump_to(&foreign, 0).unwrap_err(),
        frame
            .begin_collection_mutation(&foreign, &original)
            .unwrap_err(),
    ] {
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    }
    assert_eq!(
        frame.read_register(&runtime, Register::new(0)).unwrap(),
        original
    );
    assert_eq!(frame.instruction_offset(), 0);
    assert!(!runtime.is_quarantined());
    assert!(!foreign.is_quarantined());
    drop(frame);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&original));
    drop(stack);
    runtime.collect_garbage().unwrap();
    assert!(!runtime.gc().validate_value(&original));
}
