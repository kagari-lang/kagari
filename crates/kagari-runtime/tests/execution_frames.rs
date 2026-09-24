use kagari_ir::{
    bytecode::{
        BytecodeFunction, BytecodeInstruction, BytecodeModule, BytecodeProgram, FunctionMetadata,
        FunctionRecord, FunctionRef, ModuleRef, Register,
    },
    module::ValueType,
};
use kagari_runtime::{LoadedModule, Runtime, RuntimeErrorKind, value::Value};

#[derive(Debug)]
struct ReentrantObserver;

impl kagari_runtime::ExecutionObserver for ReentrantObserver {
    fn observe(
        &self,
        runtime: &Runtime,
        _: kagari_runtime::ExecutionEvent,
        _: &[kagari_runtime::ExecutionFrame],
    ) -> Result<(), kagari_runtime::RuntimeError> {
        let module = runtime.execution_root().unwrap();
        let nested = runtime.enter_execution_stack(&module)?;
        nested.push(module.slot(), FunctionRef::new(0), &[], None)
    }
}

fn loaded(runtime: &mut Runtime) -> LoadedModule {
    let function = BytecodeFunction {
        id: FunctionRef::new(0),
        name: "main".into(),
        register_count: 1,
        metadata: FunctionMetadata {
            registers: vec![ValueType::HeapObject],
            ..Default::default()
        },
        instructions: vec![BytecodeInstruction::Return(None)],
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
        .push(module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let first = Value::Array(runtime.alloc_array(vec![Value::I32(1)]).unwrap());
    outer
        .current_mut()
        .unwrap()
        .write_register(Register::new(0), first.clone())
        .unwrap();
    let nested = runtime.enter_execution_stack(&module).unwrap();
    nested
        .push(module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let second = Value::Array(runtime.alloc_array(vec![Value::I32(2)]).unwrap());
    nested
        .current_mut()
        .unwrap()
        .write_register(Register::new(0), second.clone())
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
                .push(module.slot(), FunctionRef::new(0), &[], None)
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
        .push(module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    assert_eq!(
        stack
            .current()
            .unwrap()
            .read_register(Register::new(9))
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
        .push(module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let nested = runtime.enter_execution_stack(&module).unwrap();
    nested
        .push(module.slot(), FunctionRef::new(0), &[], None)
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
    let stack = runtime.enter_execution_stack(&module).unwrap();
    let observer = std::rc::Rc::new(ReentrantObserver);
    assert!(runtime.attach_execution_observer(observer.clone()).unwrap());
    assert!(!runtime.attach_execution_observer(observer).unwrap());
    assert_eq!(
        runtime
            .attach_execution_observer(std::rc::Rc::new(ReentrantObserver))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    stack
        .push(module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    assert_eq!(
        runtime
            .observe_execution(kagari_runtime::ExecutionEvent::BeforeInstruction)
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
                modules: vec![(*old.bytecode).clone()],
            },
        )
        .unwrap();
    let old_object = Value::Array(runtime.alloc_array(vec![Value::I32(7)]).unwrap());
    let initialization = runtime.begin_candidate_initialization(&candidate).unwrap();
    let stack = runtime.enter_execution_stack(candidate.module()).unwrap();
    assert_eq!(
        stack
            .push(
                candidate.module().slot(),
                FunctionRef::new(0),
                &[old_object],
                None
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    stack
        .push(candidate.module().slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    assert_eq!(runtime.resources().counters().current_call_depth, 1);
    drop(outer);
    assert!(!runtime.is_quarantined());
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
        .push(old.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let candidate = runtime
        .stage_reload_program(
            &old,
            "frames",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![(*old.bytecode).clone()],
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
    assert_eq!(runtime.resources().counters().loaded_modules, 1);
    assert!(runtime.execution_root().is_none());
}
