use super::*;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime, RuntimeConfig,
    error::fault::EngineFault,
    frame::{driver::ReturnAction, transfer::ReturnValue},
    module::{
        ExecutableModule,
        managed::{
            InstallationState,
            cell::{StaticCellObject, StaticPhase},
            revision::RevisionState,
        },
    },
    native::conversion::context::ConversionContext,
    resource::RuntimeLimits,
    value::carriers::RootedValue,
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
    ProgramFixture::from(code)
        .metadata(definitions, &Default::default())
        .unwrap()
}

// The quarantine case cannot use an execution-admitted public snapshot. Inspect
// the actual publication/cell only as an independent cleanup oracle in this test.
fn physical_phase(runtime: &mut Runtime, owner: &ExecutableModule) -> StaticPhase {
    let current = runtime.modules.current.borrow();
    let root = &current.get(&owner.installation_id()).unwrap().root;
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let installation = cx.get(root)?;
            let revision = InstallationState::current(cx, installation).unwrap();
            let cell = RevisionState::static_cell(cx, revision, owner.slot(), ModuleSlot::new(0))?;
            Ok(cx.read(cell, StaticCellObject::phase))
        })
        .unwrap()
}

fn snapshot(runtime: &mut Runtime, owner: &ExecutableModule) -> StaticState {
    runtime
        .module_instance_snapshot(owner)
        .unwrap()
        .module_slots
        .remove(0)
}

#[test]
fn initializer_frame_drop_poison_survives_termination_and_releases_recipe_edges() {
    for mode in 0..4 {
        let mut runtime = Runtime::default();
        let code = access_program();
        let old_program = load_bytecode(&mut runtime, "state", code.clone()).unwrap();
        let old = runtime.select_program(&old_program).unwrap();
        let staged = runtime
            .stage_reload_program(&old_program, None, code)
            .unwrap();
        let current = staged.module().clone();
        runtime.publish_staged_reload(staged).unwrap();
        runtime.collect_garbage().unwrap();
        let baseline_roots = runtime.gc_stats().root_registrations;
        let mut stack_activation = runtime.enter_execution_stack(&current).unwrap();
        stack_activation
            .with(|runtime, stack| {
                stack
                    .push(runtime, current.slot(), FunctionRef::new(0), &[], None)
                    .unwrap();
                stack
                    .current_mut(runtime)
                    .unwrap()
                    .next_instruction()
                    .unwrap();
                assert!(
                    !stack
                        .ensure_static_ready(runtime, ModuleSlot::new(0))
                        .unwrap()
                );
                assert_eq!(stack.current(runtime).unwrap().loaded().key(), old.key());
                assert!(runtime.static_initialization_active());
                assert!(!stack.can_park(runtime).unwrap());
                assert_eq!(
                    runtime
                        .spawn_task(&RootedValue::unit(), &RootedValue::unit())
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::ExecutionPhaseViolation
                );
                assert_eq!(
                    runtime
                        .create_dispatched_task_scope(
                            &current,
                            Default::default(),
                            Arc::new(|_| Ok(()))
                        )
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::ExecutionPhaseViolation
                );
                assert_eq!(
                    stack
                        .make_future(runtime, FunctionRef::new(0), &[], Slot::new(0), &scalar())
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::ExecutionPhaseViolation
                );
                assert_eq!(
                    stack
                        .begin_await(runtime, Slot::new(0), Slot::new(0), &scalar())
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
                        runtime.quarantine_execution_invariant(EngineFault::HostInvariant(
                            "test cleanup after fault",
                        ));
                    }
                    3 => {
                        stack.fail_static_initializers(runtime, "observer failed");
                    }
                    _ => unreachable!(),
                }

                Ok(())
            })
            .unwrap();
        drop(stack_activation);
        assert!(!runtime.static_initialization_active());
        assert_eq!(physical_phase(&mut runtime, &current), StaticPhase::Failed);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
        if mode != 2 {
            let error = runtime
                .read_module_slot(&current, ModuleSlot::new(0))
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            assert!(error.to_string().contains("initializ"));
            runtime.collect_garbage().unwrap();
            assert!(runtime.modules().loaded(old.key()).is_none());
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
    let loaded_program = load_bytecode(&mut runtime, "state", access_program()).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let baseline_roots = runtime.gc_stats().root_registrations;
    let mut stack_activation = runtime.enter_execution_stack(&loaded).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push(runtime, loaded.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            stack
                .current_mut(runtime)
                .unwrap()
                .next_instruction()
                .unwrap();
            assert!(
                stack
                    .ensure_static_ready(runtime, ModuleSlot::new(0))
                    .is_err()
            );

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    assert!(matches!(
        snapshot(&mut runtime, &loaded),
        StaticState::Uninitialized(_)
    ));
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
}

#[test]
fn swallowed_cycle_cannot_commit_and_candidate_switch_cannot_escape_marker() {
    let diagnostic = StaticFailure::new(&"界".repeat(500), Some(Span::default()));
    assert!(diagnostic.cause().len() <= 1024);
    assert_eq!(diagnostic.origin(), Some(Span::default()));
    let mut runtime = Runtime::default();
    let code = access_program();
    let loaded_program = load_bytecode(&mut runtime, "state", code.clone()).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let candidate = runtime
        .stage_reload_program(&loaded_program, None, code)
        .unwrap();
    let baseline_roots = runtime.gc_stats().root_registrations;
    let mut stack_activation = runtime.enter_execution_stack(&loaded).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push(runtime, loaded.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            stack
                .current_mut(runtime)
                .unwrap()
                .next_instruction()
                .unwrap();
            assert!(
                !stack
                    .ensure_static_ready(runtime, ModuleSlot::new(0))
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
                    .finish_return(runtime, ReturnValue::retained(RootedValue::from(42)))
                    .is_err()
            );

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    let StaticState::Failed(failure) = snapshot(&mut runtime, &loaded) else {
        panic!("poisoned")
    };
    assert!(failure.cause().contains("recursive"));
    assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn initializer_return_checks_semantic_type_before_ready_and_retries_access_pc() {
    for valid in [false, true] {
        let mut runtime = Runtime::default();
        let code = access_program();
        let loaded_program = load_bytecode(&mut runtime, "state", code.clone()).unwrap();
        let old = runtime.select_program(&loaded_program).unwrap();
        let candidate = runtime
            .stage_reload_program(&loaded_program, None, code)
            .unwrap();
        let loaded = candidate.module().clone();
        runtime.publish_staged_reload(candidate).unwrap();
        runtime.collect_garbage().unwrap();
        let baseline_roots = runtime.gc_stats().root_registrations;
        let mut stack_activation = runtime.enter_execution_stack(&loaded).unwrap();
        stack_activation
            .with(|runtime, stack| {
                stack
                    .push(runtime, loaded.slot(), FunctionRef::new(0), &[], None)
                    .unwrap();
                stack
                    .current_mut(runtime)
                    .unwrap()
                    .next_instruction()
                    .unwrap();
                assert!(
                    !stack
                        .ensure_static_ready(runtime, ModuleSlot::new(0))
                        .unwrap()
                );
                assert_eq!(stack.current(runtime).unwrap().loaded().key(), old.key());
                let result = stack.finish_return(
                    runtime,
                    ReturnValue::retained(if valid {
                        RootedValue::from(42)
                    } else {
                        RootedValue::from(true)
                    }),
                );
                if valid {
                    assert!(matches!(result, Ok(ReturnAction::Continue(_))));
                    assert!(!runtime.static_initialization_active());
                    stack.current_mut(runtime).unwrap().prepare_instruction();
                    assert!(matches!(
                        stack.current_mut(runtime).unwrap().next_instruction(),
                        Some(BytecodeInstruction::LoadModule { .. })
                    ));
                    assert!(
                        stack
                            .ensure_static_ready(runtime, ModuleSlot::new(0))
                            .unwrap()
                    );
                    runtime.collect_garbage().unwrap();
                    // Clearing the recipe and retiring its frame releases the old
                    // body even while the current caller remains admitted.
                    assert!(runtime.modules().loaded(old.key()).is_none());
                    let value = runtime
                        .read_module_slot(&loaded, ModuleSlot::new(0))
                        .unwrap();
                    let mut conversion = ConversionContext::new(runtime, &loaded).unwrap();
                    assert_eq!(conversion.decode::<i32>(&value).unwrap(), 42);
                    drop(conversion);
                    assert!(
                        runtime
                            .write_module_slot(&loaded, ModuleSlot::new(0), &RootedValue::from(9))
                            .is_err()
                    );
                } else {
                    assert!(result.is_err());
                }

                Ok(())
            })
            .unwrap();
        drop(stack_activation);
        assert!(
            matches!(snapshot(&mut runtime, &loaded), StaticState::Ready(_) if valid)
                || matches!(snapshot(&mut runtime, &loaded), StaticState::Failed(_) if !valid)
        );
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
    }
}
