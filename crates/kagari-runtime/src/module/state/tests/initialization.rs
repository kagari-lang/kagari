use super::*;
use crate::{
    RuntimeConfig,
    frame::{driver::ReturnAction, transfer::ReturnValue},
    resource::RuntimeLimits,
};
use std::sync::Arc;

fn access_program() -> DefinitionMetadata<BytecodeProgram> {
    let (definitions, mut code) = program(scalar(), false).into_parts();
    let main = &mut code.modules[0].functions[0];
    main.metadata.slots = vec![ValueType::I32];
    main.metadata.roots = RootSlotLayout::from_types(&[ValueType::I32]);
    main.metadata.semantic.slots.insert(0, scalar());
    main.instructions = vec![
        BytecodeInstruction::LoadModule {
            dst: Slot::new(0),
            slot: ModuleSlot::new(0),
        },
        BytecodeInstruction::Return(None),
    ];
    let mut origin = main.metadata.origins[0].clone();
    origin.instruction = 1;
    main.metadata.origins.push(origin);
    DefinitionMetadata::checked(definitions, code, &Default::default()).unwrap()
}

#[test]
fn initializer_frame_drop_poison_survives_termination_and_releases_recipe_edges() {
    for mode in 0..4 {
        let mut runtime = Runtime::default();
        let code = access_program();
        let old = runtime.load_program("state", code.clone()).unwrap();
        let staged = runtime.stage_reload_program(&old, "state", code).unwrap();
        let current = runtime.publish_staged_reload(staged).unwrap();
        let stack = runtime.enter_execution_stack(&current).unwrap();
        stack
            .push(&runtime, current.slot(), FunctionRef::new(0), &[], None)
            .unwrap();
        stack.current_mut().unwrap().next_instruction().unwrap();
        assert!(
            !stack
                .ensure_static_ready(&runtime, ModuleSlot::new(0))
                .unwrap()
        );
        assert_eq!(stack.current().unwrap().loaded().key(), old.key());
        assert!(runtime.static_initialization_active());
        assert!(!stack.can_park(&runtime).unwrap());
        assert_eq!(
            runtime
                .spawn_task(&Value::Unit, &Value::Unit)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            runtime
                .create_task_scope(&current, Default::default(), Arc::new(|_| Ok(())))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            stack
                .make_future(&runtime, FunctionRef::new(0), vec![], &scalar())
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            stack
                .begin_await(&runtime, Value::Unit, Slot::new(0), &scalar())
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        match mode {
            0 => {}
            1 => {
                runtime.execution_options().cancellation.cancel();
            }
            2 => {
                runtime.quarantine_execution_invariant("test cleanup after fault");
            }
            3 => {
                stack.fail_static_initializers("observer failed");
            }
            _ => unreachable!(),
        }
        drop(stack);
        assert!(!runtime.static_initialization_active());
        assert!(matches!(
            runtime.modules.static_state(&current, ModuleSlot::new(0)),
            Some(StaticState::Failed(_))
        ));
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc().active_roots(), 0);
        if mode != 2 {
            let error = runtime
                .read_module_slot(&current, ModuleSlot::new(0))
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            assert!(error.message().contains("initializ"));
            assert!(
                runtime
                    .collect_garbage()
                    .unwrap()
                    .reclaimed_modules
                    .contains(&old.key())
            );
        }
    }
}

#[test]
fn pre_attempt_depth_rejection_leaves_cell_uninitialized() {
    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            max_call_depth: Some(1),
        },
        ..Default::default()
    });
    let loaded = runtime.load_program("state", access_program()).unwrap();
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push(&runtime, loaded.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    stack.current_mut().unwrap().next_instruction().unwrap();
    assert!(
        stack
            .ensure_static_ready(&runtime, ModuleSlot::new(0))
            .is_err()
    );
    drop(stack);
    assert!(matches!(
        runtime.modules.static_state(&loaded, ModuleSlot::new(0)),
        Some(StaticState::Uninitialized(_))
    ));
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn swallowed_cycle_cannot_commit_and_candidate_switch_cannot_escape_marker() {
    let mut runtime = Runtime::default();
    let code = access_program();
    let loaded = runtime.load_program("state", code.clone()).unwrap();
    let candidate = runtime
        .stage_reload_program(&loaded, "state", code)
        .unwrap();
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push(&runtime, loaded.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    stack.current_mut().unwrap().next_instruction().unwrap();
    assert!(
        !stack
            .ensure_static_ready(&runtime, ModuleSlot::new(0))
            .unwrap()
    );
    assert!(runtime.begin_candidate_initialization(&candidate).is_err());
    assert!(
        runtime
            .read_module_slot(&loaded, ModuleSlot::new(0))
            .is_err()
    );
    assert!(
        stack
            .finish_return(&runtime, ReturnValue::general(Value::I32(42)))
            .is_err()
    );
    drop(stack);
    let StaticState::Failed(failure) = runtime
        .modules
        .static_state(&loaded, ModuleSlot::new(0))
        .unwrap()
    else {
        panic!("poisoned")
    };
    assert!(failure.cause().contains("recursive"));
}

#[test]
fn initializer_return_failure_after_retirement_poison_is_owned_by_the_attempt() {
    let mut runtime = Runtime::default();
    let loaded = runtime.load_program("state", access_program()).unwrap();
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push(&runtime, loaded.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    stack.current_mut().unwrap().next_instruction().unwrap();
    assert!(
        !stack
            .ensure_static_ready(&runtime, ModuleSlot::new(0))
            .unwrap()
    );
    let held = runtime.modules.inner.borrow();
    let error = stack
        .finish_return(&runtime, ReturnValue::general(Value::I32(42)))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    drop(held);
    drop(stack);
    let StaticState::Failed(failure) = runtime
        .modules
        .static_state(&loaded, ModuleSlot::new(0))
        .unwrap()
    else {
        panic!("return must not publish Ready")
    };
    assert!(
        failure
            .cause()
            .contains("static store borrowed during initializer return")
    );
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn initializer_return_checks_semantic_type_before_ready_and_retries_access_pc() {
    for valid in [false, true] {
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("state", access_program()).unwrap();
        let stack = runtime.enter_execution_stack(&loaded).unwrap();
        stack
            .push(&runtime, loaded.slot(), FunctionRef::new(0), &[], None)
            .unwrap();
        stack.current_mut().unwrap().next_instruction().unwrap();
        assert!(
            !stack
                .ensure_static_ready(&runtime, ModuleSlot::new(0))
                .unwrap()
        );
        let result = stack.finish_return(
            &runtime,
            ReturnValue::general(if valid {
                Value::I32(42)
            } else {
                Value::Bool(true)
            }),
        );
        if valid {
            assert!(matches!(result, Ok(ReturnAction::Continue(_))));
            assert!(!runtime.static_initialization_active());
            stack.current_mut().unwrap().prepare_instruction();
            assert!(matches!(
                stack.current_mut().unwrap().next_instruction(),
                Some(BytecodeInstruction::LoadModule { .. })
            ));
            assert!(
                stack
                    .ensure_static_ready(&runtime, ModuleSlot::new(0))
                    .unwrap()
            );
            assert_eq!(
                runtime
                    .read_module_slot(&loaded, ModuleSlot::new(0))
                    .unwrap(),
                Value::I32(42)
            );
            assert!(
                runtime
                    .write_module_slot(&loaded, ModuleSlot::new(0), Value::I32(9))
                    .is_err()
            );
        } else {
            assert!(result.is_err());
        }
        drop(stack);
        assert!(
            matches!(runtime.modules.static_state(&loaded, ModuleSlot::new(0)).unwrap(), StaticState::Ready(_) if valid)
                || matches!(runtime.modules.static_state(&loaded, ModuleSlot::new(0)).unwrap(), StaticState::Failed(_) if !valid)
        );
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}
