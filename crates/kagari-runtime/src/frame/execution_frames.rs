#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime, RuntimeConfig,
    error::{
        RuntimeError, RuntimeErrorKind,
        reason::{CancellationReason, ErrorReason},
    },
    execution::roots::ExecutionRoots,
    executor::native,
    frame::{ExecutionFrame, FrameDispatch, FrameEntry, FrameStack, arguments::FrameArguments},
    jit_abi::jit_poll_execution,
    module::{
        ExecutableModule, InstalledProgram,
        execution::{layout::Location, operands::OperandSlot},
        managed::source::RevisionSource,
    },
    native::conversion::context::ConversionContext,
    resource::RuntimeLimits,
    session::{ExecutionEntry, ExecutionEvent, ExecutionObserver},
    test_support::native::program::ProgramFixture,
    value::{arrays::ArrayObject, carriers::RootedValue, scalar, view::ValueView},
};
use std::num::NonZeroUsize;

use kagari_abi::{
    native_call::{JIT_STATUS_CANCELLED, JIT_STATUS_ENGINE_FAULT, JIT_STATUS_OK},
    representation::ValueType,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, JumpTarget, Slot},
    module::{
        BytecodeFunction, BytecodeModule, BytecodeSemanticSlots, CallableTarget, FunctionMetadata,
        FunctionRecord, InstructionOrigin, RootSlotLayout,
    },
    program::ModuleRef,
};
use kagari_common::{
    cancellation::CancellationToken, identity::map::DefinitionContext, span::Span,
};
use kagari_contract::ids::FunctionRef;
use kagari_gc::error::GcError;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use rstest::rstest;
use std::slice;

mod origins;
mod slots;

#[derive(Debug)]
struct InspectingObserver;

impl ExecutionObserver for InspectingObserver {
    fn observe(
        &mut self,
        runtime: &Runtime,
        _: ExecutionEvent,
        frames: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].loaded().key(),
            runtime.execution_root().unwrap().key()
        );
        assert_eq!(runtime.resources().counters().current_call_depth, 1);
        Ok(())
    }
}

pub(super) fn loaded(runtime: &mut Runtime) -> (InstalledProgram, ExecutableModule) {
    loaded_with_instructions(runtime, vec![BytecodeInstruction::Return(None)])
}

fn loaded_with_instructions(
    runtime: &mut Runtime,
    instructions: Vec<BytecodeInstruction>,
) -> (InstalledProgram, ExecutableModule) {
    let function = {
        let fixture_instructions = instructions;
        let mut fixture_metadata = FunctionMetadata {
            roots: RootSlotLayout::from_types(
                &[[ValueType::HeapObject].to_vec(), [].to_vec()].concat(),
            ),
            slots: [vec![ValueType::HeapObject], vec![].clone()].concat(),
            ..Default::default()
        };

        if fixture_metadata.origins.is_empty() {
            fixture_metadata.origins = (0..fixture_instructions.len())
                .map(|instruction| InstructionOrigin {
                    span: Default::default(),
                    line: None,
                    column: None,
                    instruction,
                })
                .collect();
        }
        BytecodeFunction {
            id: FunctionRef::new(0),
            name: "main".into(),
            slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
            metadata: fixture_metadata,
            instructions: fixture_instructions,
            ..Default::default()
        }
    };
    loaded_function(runtime, function)
}

fn loaded_function(
    runtime: &mut Runtime,
    function: BytecodeFunction,
) -> (InstalledProgram, ExecutableModule) {
    let context = DefinitionContext::new().unwrap();
    let program = load_bytecode(
        runtime,
        "frames",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                types: vec![ValueType::Unit, ValueType::HeapObject],
                function_table: vec![FunctionRecord {
                    id: function.id,
                    identity: function.identity.clone(),
                    name: function.name.clone(),
                    params: function.metadata.params.clone(),
                    return_type: function.metadata.return_type,
                    effects: Default::default(),
                }],
                functions: vec![function],
                ..Default::default()
            }],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let selected = runtime.select_program(&program).unwrap();
    (program, selected)
}

#[rstest]
#[case::foreign_heap(
    false,
    false,
    None,
    2,
    RuntimeError::module_validation(GcError::ForeignHeap.to_string())
)]
#[case::arity_before_heap(
    false,
    false,
    None,
    1,
    RuntimeError::module_validation("frame argument count does not match the linked function")
)]
#[case::cancel_before_heap(
    false,
    true,
    None,
    2,
    RuntimeError::from(CancellationReason::ExecutionCancelled)
)]
#[case::space_before_heap(
    false,
    false,
    Some(256),
    2,
    RuntimeError::new(ErrorReason::StackOverflow)
)]
#[case::candidate_before_cancel(
    true,
    true,
    None,
    2,
    RuntimeError::execution_phase_violation("external object in candidate call arguments")
)]
#[case::candidate_before_space(
    true,
    false,
    Some(256),
    2,
    RuntimeError::execution_phase_violation("external object in candidate call arguments")
)]
fn call_argument_admission_preserves_failure_precedence_and_atomicity(
    #[case] candidate_phase: bool,
    #[case] cancel: bool,
    #[case] stack_bytes: Option<usize>,
    #[case] argument_count: usize,
    #[case] expected: RuntimeError,
) {
    let mut foreign = Runtime::default();
    let (_foreign_module_program, foreign_module) = loaded(&mut foreign);
    let foreign_value = foreign
        .alloc_array(&foreign_module, Ty::Builtin(BuiltinType::I32), &[])
        .unwrap();

    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            max_stack_bytes: NonZeroUsize::new(stack_bytes.unwrap_or(8 * 1024 * 1024)).unwrap(),
            ..Default::default()
        },
        ..Default::default()
    });
    let params = vec![ValueType::HeapObject; 2];
    let (baseline_program, baseline) = loaded_function(&mut runtime, {
        let parameter_base: usize = 0;
        let fixture_instructions = vec![BytecodeInstruction::Return(None)];
        let mut fixture_metadata = FunctionMetadata {
            roots: RootSlotLayout::from_types(&[[].to_vec(), params.to_vec()].concat()),
            slots: [vec![], params.clone()].concat(),
            params,
            ..Default::default()
        };
        for index in 0..2 {
            fixture_metadata
                .semantic
                .params
                .insert(index, Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))));
        }
        if fixture_metadata.parameter_slots.is_empty() {
            fixture_metadata.parameter_slots =
                (0..2).map(|i| Slot::new(parameter_base + i)).collect();
        }
        if fixture_metadata.origins.is_empty() {
            fixture_metadata.origins = (0..fixture_instructions.len())
                .map(|instruction| InstructionOrigin {
                    span: Default::default(),
                    line: None,
                    column: None,
                    instruction,
                })
                .collect();
        }
        BytecodeFunction {
            id: FunctionRef::new(0),
            name: "main".into(),
            parameter_count: 2,
            slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
            metadata: fixture_metadata,
            instructions: fixture_instructions,
            ..Default::default()
        }
    });
    let candidate = candidate_phase.then(|| {
        let (definitions, module) = baseline
            .to_unverified(&Default::default())
            .unwrap()
            .into_parts();
        runtime
            .stage_reload_program(
                &baseline_program,
                None,
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![module],
                }
                .metadata(definitions, &Default::default())
                .unwrap(),
            )
            .unwrap()
    });
    let old_object = runtime
        .alloc_array(&baseline, Ty::Builtin(BuiltinType::I32), &[])
        .unwrap();
    let mut foreign_source = ConversionContext::new(&mut foreign, &foreign_module).unwrap();
    let foreign_view = foreign_source.import(&foreign_value).unwrap();
    // Protect the old input before candidate entry so the frame's candidate
    // policy, rather than Host import, decides the rejection and its precedence.
    let mut source = ConversionContext::new(&mut runtime, &baseline).unwrap();
    let old_view = source.import(&old_object).unwrap();
    let check = |runtime: &mut Runtime, owner: &ExecutableModule| {
        let cancellation = runtime.execution_options().cancellation;
        let mut stack_activation = runtime.enter_execution_stack(owner).unwrap();
        stack_activation
            .with(|runtime, stack| {
                let roots_before = runtime.gc_stats().root_registrations;
                if cancel {
                    cancellation.cancel();
                }
                let arguments = if candidate_phase {
                    [ValueView::unit(), old_view]
                } else {
                    [foreign_view, ValueView::unit()]
                };
                let error = stack
                    .push(
                        runtime,
                        owner.slot(),
                        FunctionRef::new(0),
                        &arguments[..argument_count],
                        None,
                    )
                    .unwrap_err();
                assert_eq!(error.kind(), expected.kind());
                assert_eq!(error.to_string(), expected.to_string());
                assert!(stack.is_empty(runtime).unwrap());
                assert_eq!(runtime.resources().counters().current_call_depth, 0);
                assert_eq!(runtime.gc_stats().root_registrations, roots_before);
                assert!(!runtime.is_quarantined());

                Ok(())
            })
            .unwrap();
    };
    if let Some(candidate) = &candidate {
        let mut initialization = source
            .runtime_mut()
            .begin_candidate_initialization(candidate)
            .unwrap();
        check(initialization.runtime_mut(), candidate.module());
    } else {
        let options = source.runtime().execution_options();
        let mut session = source
            .runtime_mut()
            .begin_execution(&baseline, options)
            .unwrap();
        check(session.runtime_mut(), &baseline);
    }
    drop(source);
    drop(foreign_source);
}

#[test]
fn invalid_prepared_argument_faults_without_publishing_another_frame() {
    let mut runtime = Runtime::default();
    let (_program, module) = loaded_function(
        &mut runtime,
        BytecodeFunction {
            name: "main".into(),
            parameter_count: 1,
            slot_count: 1,
            metadata: FunctionMetadata {
                semantic: BytecodeSemanticSlots {
                    params: [(0, Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))))].into(),
                    ..Default::default()
                },
                slots: vec![ValueType::HeapObject],
                params: vec![ValueType::HeapObject],
                parameter_slots: vec![Slot::new(0)],
                roots: RootSlotLayout::from_types(&[ValueType::HeapObject]),
                origins: vec![InstructionOrigin {
                    instruction: 0,
                    span: Default::default(),
                    line: None,
                    column: None,
                }],
                ..Default::default()
            },
            instructions: vec![BytecodeInstruction::Return(None)],
            ..Default::default()
        },
    );
    let input = runtime
        .alloc_array(&module, Ty::Builtin(BuiltinType::I32), &[])
        .unwrap();
    let roots_before = runtime.gc_stats().root_registrations;
    let mut activation = runtime.enter_execution_stack(&module).unwrap();
    activation
        .with(|runtime, stack| {
            stack
                .push_rooted_callable(
                    runtime,
                    module.clone(),
                    CallableTarget::Script(FunctionRef::new(0)),
                    slice::from_ref(&input),
                )
                .unwrap();
            let slots = stack.current(runtime).unwrap().slots();
            let sources = [Location {
                operand: OperandSlot::new(1, true),
                representation: ValueType::HeapObject,
                semantic: None,
                borrowed_receiver: false,
            }];
            let error = stack
                .push_admitted_arguments(
                    runtime,
                    module.clone(),
                    CallableTarget::Script(FunctionRef::new(0)),
                    FrameArguments::frame(slots, &sources),
                    None,
                    FrameDispatch {
                        origin: RevisionSource::Execution(slots.body),
                        metadata: None,
                        prepared: None,
                        entry: FrameEntry::Call,
                        invocation: None,
                        environment: None,
                    },
                )
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
            assert!(error.to_string().contains(&GcError::Index.to_string()));
            assert!(runtime.is_quarantined());
            assert_eq!(stack.frames(runtime).unwrap().len(), 1);
            assert_eq!(runtime.resources().counters().current_call_depth, 1);
            assert_eq!(runtime.gc_stats().root_registrations, roots_before);
            Ok(())
        })
        .unwrap();
    drop(activation);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc_stats().root_registrations, roots_before);
}

fn set_frame_array(runtime: &mut Runtime, stack: &FrameStack, owner: &ExecutableModule, item: i32) {
    let array = runtime
        .alloc_array(
            owner,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(item)],
        )
        .unwrap();
    let mut source = ConversionContext::new(runtime, owner).unwrap();
    let value = source.import(&array).unwrap();
    stack
        .write_slot(source.runtime_mut(), Slot::new(0), &value)
        .unwrap();
    drop(source);
    drop(array);
}

fn frame_array_item(runtime: &mut Runtime, stack: &FrameStack, index: usize) -> i128 {
    let location = stack.frames(runtime).unwrap()[index]
        .read_slot(runtime, Slot::new(0))
        .unwrap()
        .location()
        .unwrap();
    // The actual frame is still admitted throughout this closed source read.
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = slots.get(location)?;
                let array = slots
                    .scope()
                    .downcast::<ArrayObject>(value.object().unwrap())?;
                let item = ArrayObject::get(slots.scope(), array, 0)?;
                Ok(scalar::read_integer(BuiltinType::I32, item).unwrap())
            })
        })
        .unwrap()
}

#[test]
fn nested_scopes_share_one_stack_and_unwind_only_their_own_roots() {
    let mut runtime = Runtime::default();
    let (_module_program, module) = loaded(&mut runtime);
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut outer = runtime.enter_execution_stack(&module).unwrap();
    outer
        .with(|runtime, stack| stack.push(runtime, module.slot(), FunctionRef::new(0), &[], None))
        .unwrap();
    outer
        .with(|runtime, outer| {
            set_frame_array(runtime, outer, &module, 1);
            let mut nested_activation = runtime.enter_execution_stack(&module).unwrap();
            let nested_objects = nested_activation
                .with(|runtime, nested| {
                    nested
                        .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                        .unwrap();
                    set_frame_array(runtime, nested, &module, 2);
                    assert_eq!(outer.frames(runtime)?.len(), 2);
                    assert_eq!(nested.frames(runtime)?[0].loaded().key(), module.key());
                    runtime.collect_garbage().unwrap();
                    assert_eq!(
                        runtime.gc_stats().root_registrations,
                        baseline.root_registrations
                    );
                    assert_eq!(frame_array_item(runtime, nested, 0), 1);
                    assert_eq!(frame_array_item(runtime, nested, 1), 2);
                    Ok(runtime.gc_stats().objects)
                })
                .unwrap();
            drop(nested_activation);
            assert_eq!(outer.frames(runtime)?.len(), 1);
            assert_eq!(runtime.resources().counters().current_call_depth, 1);
            runtime.collect_garbage().unwrap();
            assert_eq!(frame_array_item(runtime, outer, 0), 1);
            assert_eq!(runtime.gc_stats().objects, nested_objects - 1);
            assert_eq!(
                runtime.gc_stats().root_registrations,
                baseline.root_registrations
            );
            Ok(())
        })
        .unwrap();
    drop(outer);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(runtime.execution_root().is_none());
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert!(!runtime.is_quarantined());
}

#[test]
fn root_depth_peaks_follow_actual_frames_and_reset_between_roots() {
    let mut runtime = Runtime::default();
    let (_module_program, module) = loaded(&mut runtime);
    for depth in [2, 1] {
        let mut session = runtime
            .begin_execution(&module, runtime.execution_options())
            .unwrap();
        let mut stack_activation = session
            .runtime_mut()
            .enter_execution_stack(&module)
            .unwrap();
        stack_activation
            .with(|runtime, stack| {
                for _ in 0..depth {
                    stack
                        .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                        .unwrap();
                }
                assert_eq!(runtime.resources().counters().current_call_depth, depth);

                Ok(())
            })
            .unwrap();
        drop(stack_activation);
        assert_eq!(session.counters().current_call_depth, 0);
        assert_eq!(session.counters().peak_call_depth, depth);
    }
    assert_eq!(runtime.resources().counters().peak_call_depth, 2);
}

#[test]
fn invalid_frame_access_quarantines_but_scope_cleanup_still_releases_every_root() {
    let mut runtime = Runtime::default();
    let (_module_program, module) = loaded(&mut runtime);
    let roots_before = runtime.gc_stats().root_registrations;
    let mut stack_activation = runtime.enter_execution_stack(&module).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            assert_eq!(
                stack
                    .current(runtime)
                    .unwrap()
                    .read_slot(runtime, Slot::new(9))
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::EngineFault
            );
            assert!(runtime.is_quarantined());

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc_stats().root_registrations, roots_before);
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
    let (_module_program, module) = loaded(&mut runtime);
    let roots_before = runtime.gc_stats().root_registrations;
    let mut session = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    let id = session.scope.id;
    {
        let runtime = session.runtime_mut();
        let mut outer = FrameStack::new(runtime, id).unwrap();
        outer
            .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
            .unwrap();
        let mut nested = FrameStack::new(runtime, id).unwrap();
        nested
            .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
            .unwrap();
        outer.retire(runtime);
        assert!(runtime.is_quarantined());
        assert!(nested.current(runtime).is_err());
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc_stats().root_registrations, roots_before);
        nested.retire(runtime);
    }
    drop(session);
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
fn observers_inspect_frames_without_replacing_the_root_observer() {
    let mut runtime = Runtime::default();
    let (_module_program, module) = loaded(&mut runtime);
    let roots_before = runtime.gc_stats().root_registrations;
    runtime.set_execution_observer(InspectingObserver).unwrap();
    let mut stack_activation = runtime.enter_execution_stack(&module).unwrap();
    stack_activation
        .with(|runtime, stack| {
            assert!(runtime.attach_execution_observer().unwrap());
            assert!(!runtime.attach_execution_observer().unwrap());
            assert_eq!(
                runtime
                    .set_execution_observer(InspectingObserver)
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::ModuleValidation
            );
            stack
                .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            runtime
                .observe_execution(ExecutionEvent::BeforeInstruction)
                .unwrap();

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    assert!(!runtime.is_quarantined());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc_stats().root_registrations, roots_before);
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
        fn begin(&mut self, _: &Runtime, _: &ExecutableModule) -> Result<(), RuntimeError> {
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
    let (_module_program, module) = loaded(&mut runtime);
    runtime.set_execution_observer(Probe::default()).unwrap();
    let mut stack_activation = runtime.enter_execution_stack(&module).unwrap();
    stack_activation
        .with(|runtime, _stack| {
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

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
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
    let (old_program, old) = loaded(&mut runtime);
    let mut outer = runtime
        .admit_execution(
            &old,
            runtime.execution_options(),
            ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();
    let (definitions, module) = old.to_unverified(&Default::default()).unwrap().into_parts();
    let candidate = runtime
        .stage_reload_program(
            &old_program,
            None,
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module],
            }
            .metadata(definitions, &Default::default())
            .unwrap(),
        )
        .unwrap();
    let old_object = runtime
        .alloc_array(
            &old,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(7i32)],
        )
        .unwrap();
    let mut initialization = runtime.begin_candidate_initialization(&candidate).unwrap();
    let mut stack_activation = initialization
        .runtime_mut()
        .enter_execution_stack(candidate.module())
        .unwrap();
    stack_activation
        .with(|runtime, stack| {
            assert_eq!(
                stack
                    .push_rooted_callable(
                        runtime,
                        candidate.module().clone(),
                        CallableTarget::Script(FunctionRef::new(0)),
                        slice::from_ref(&old_object),
                    )
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::ExecutionPhaseViolation
            );
            assert_eq!(runtime.resources().counters().current_call_depth, 0);
            stack
                .push(
                    runtime,
                    candidate.module().slot(),
                    FunctionRef::new(0),
                    &[],
                    None,
                )
                .unwrap();
            assert_eq!(runtime.resources().counters().current_call_depth, 1);
            assert_eq!(stack.frames(runtime).unwrap().len(), 1);
            outer.retire(runtime);
            assert!(!runtime.is_quarantined());
            assert_eq!(stack.frames(runtime).unwrap().len(), 1);
            assert_eq!(
                runtime.modules().retention_counts(old.key()).active_calls,
                0
            );
            assert!(stack.current(runtime).is_ok());

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    drop(initialization);
    assert!(runtime.execution_root().is_none());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    runtime.publish_staged_reload(candidate).unwrap();
}

#[test]
fn suspended_session_frames_cannot_be_used_during_candidate_initialization() {
    let mut runtime = Runtime::default();
    let (old_program, old) = loaded(&mut runtime);
    let mut outer_activation = runtime.enter_execution_stack(&old).unwrap();
    outer_activation
        .with(|runtime, outer| {
            outer
                .push(runtime, old.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            let (definitions, module) =
                old.to_unverified(&Default::default()).unwrap().into_parts();
            let candidate = runtime
                .stage_reload_program(
                    &old_program,
                    None,
                    ProgramFixture {
                        exports: Default::default(),
                        root: ModuleRef::new(0),
                        modules: vec![module],
                    }
                    .metadata(definitions, &Default::default())
                    .unwrap(),
                )
                .unwrap();
            let mut initialization = runtime.begin_candidate_initialization(&candidate).unwrap();
            assert_eq!(
                outer
                    .current(initialization.runtime_mut())
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::EngineFault
            );
            assert!(initialization.runtime_mut().is_quarantined());
            drop(initialization);
            drop(candidate);

            Ok(())
        })
        .unwrap();
    drop(outer_activation);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.modules().loaded_count(), 1);
    assert!(runtime.execution_root().is_none());
}

#[test]
fn native_polling_preserves_cancellation_offsets_and_cleanup() {
    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    });
    let instructions = vec![
        BytecodeInstruction::Jump {
            target: JumpTarget::new(1),
        },
        BytecodeInstruction::Jump {
            target: JumpTarget::new(1),
        },
        BytecodeInstruction::Return(None),
    ];
    let origins = [0, 1, 1, 2]
        .into_iter()
        .enumerate()
        .map(|(origin, instruction)| InstructionOrigin {
            instruction,
            span: Span::new(origin * 10, origin * 10 + 2),
            line: Some(origin as u32 + 1),
            column: Some(1),
        })
        .collect();
    let (_module_program, module) = loaded_function(
        &mut runtime,
        BytecodeFunction {
            name: "main".into(),
            metadata: FunctionMetadata {
                origins,
                instruction_origins: vec![2],
                ..Default::default()
            },
            instructions,
            ..Default::default()
        },
    );
    let roots_before = runtime.gc_stats().root_registrations;
    let token = CancellationToken::default();
    let mut options = runtime.execution_options();
    options.cancellation = token.clone();
    let mut _session = runtime.begin_execution(&module, options).unwrap();
    let mut stack_activation = _session
        .runtime_mut()
        .enter_execution_stack(&module)
        .unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            for origin in 0..3 {
                assert_eq!(
                    unsafe { jit_poll_execution(runtime, origin) },
                    JIT_STATUS_OK
                );
            }
            token.cancel();
            assert_eq!(
                unsafe { jit_poll_execution(runtime, 3) },
                JIT_STATUS_CANCELLED
            );
            let trace = runtime.capture_error_trace();
            assert_eq!(trace.frames[0].instruction_offset, 2);
            assert_eq!(trace.frames[0].semantic_origin, Some(3));
            assert_eq!(trace.frames[0].source_span, Some(Span::new(30, 32)));
            assert_eq!(
                (trace.frames[0].line, trace.frames[0].column),
                (Some(4), Some(1))
            );

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    assert_eq!(
        _session.runtime().resources().counters().current_call_depth,
        0
    );
    assert_eq!(
        _session.runtime().gc_stats().root_registrations,
        roots_before
    );
}

#[test]
fn native_polling_requires_an_active_validated_program_point() {
    let mut runtime = Runtime::default();
    assert_eq!(
        unsafe { jit_poll_execution(&mut runtime, 0) },
        JIT_STATUS_ENGINE_FAULT
    );

    let mut runtime = Runtime::default();
    let (_module_program, module) = loaded(&mut runtime);
    let mut stack_activation = runtime.enter_execution_stack(&module).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            assert_eq!(
                unsafe { jit_poll_execution(runtime, 1) },
                JIT_STATUS_ENGINE_FAULT
            );

            Ok(())
        })
        .unwrap();
}

#[test]
fn interpreter_frame_fetch_preserves_instruction_offsets() {
    let mut runtime = Runtime::default();
    let (_module_program, module) = loaded_with_instructions(
        &mut runtime,
        vec![
            BytecodeInstruction::Jump {
                target: JumpTarget::new(1),
            },
            BytecodeInstruction::Return(None),
        ],
    );
    let mut stack_activation = runtime.enter_execution_stack(&module).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            for index in 0..2 {
                let instruction = stack
                    .current_mut(runtime)
                    .unwrap()
                    .next_instruction()
                    .unwrap();
                assert_eq!(stack.current(runtime).unwrap().instruction_offset(), index);
                assert_eq!(
                    matches!(instruction, BytecodeInstruction::Jump { .. }),
                    index == 0
                );
                runtime.resources().poll_execution().unwrap();
            }

            assert!(
                stack
                    .current_mut(runtime)
                    .unwrap()
                    .next_instruction()
                    .is_none()
            );

            Ok(())
        })
        .unwrap();
}

#[test]
fn frame_access_rejects_a_foreign_context_before_changing_slots_or_program_point() {
    let mut runtime = Runtime::default();
    let (_module_program, module) = loaded(&mut runtime);
    let mut foreign = Runtime::default();
    let original = runtime
        .alloc_array(
            &module,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(3i32)],
        )
        .unwrap();
    let mut stack_activation = runtime.enter_execution_stack(&module).unwrap();
    stack_activation
        .with(|runtime, stack| {
            assert_eq!(
                stack
                    .push(&mut foreign, module.slot(), FunctionRef::new(0), &[], None)
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::ModuleValidation
            );
            assert!(stack.is_empty(runtime).unwrap());
            assert_eq!(runtime.resources().counters().current_call_depth, 0);
            stack
                .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            let mut conversion = ConversionContext::new(runtime, &module).unwrap();
            let input = conversion.import(&original).unwrap();
            stack
                .write_slot(conversion.runtime_mut(), Slot::new(0), &input)
                .unwrap();
            drop(conversion);
            for error in [
                stack.current(&foreign).unwrap_err(),
                stack.current_mut(&foreign).unwrap_err(),
                stack.frames(&foreign).unwrap_err(),
                RuntimeError::from(
                    stack
                        .execute_region(&mut foreign, &mut None, native::invoke_script)
                        .map(|_| ())
                        .unwrap_err(),
                ),
                stack
                    .write_slot(&mut foreign, Slot::new(0), &ValueView::unit())
                    .unwrap_err(),
            ] {
                assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            }
            let mut frame = stack.current_mut(runtime).unwrap();
            for error in [
                frame.read_slot(&foreign, Slot::new(0)).unwrap_err(),
                frame.jump_to(&foreign, 0).unwrap_err(),
            ] {
                assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            }
            let location = frame
                .read_slot(runtime, Slot::new(0))
                .unwrap()
                .location()
                .unwrap();
            assert_eq!(frame.instruction_offset(), 0);
            drop(frame);
            assert!(!runtime.is_quarantined());
            assert!(!foreign.is_quarantined());
            drop(original);
            runtime.collect_garbage().unwrap();
            runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let value = slots.get(location)?;
                        let array = slots
                            .scope()
                            .downcast::<ArrayObject>(value.object().unwrap())?;
                        let value = ArrayObject::get(slots.scope(), array, 0)?;
                        assert_eq!(scalar::read_integer(BuiltinType::I32, value).unwrap(), 3);
                        Ok(())
                    })
                })
                .unwrap();
            Ok(())
        })
        .unwrap();
    let before = stack_activation
        .with(|runtime, _| Ok(runtime.memory.stats().objects))
        .unwrap();
    drop(stack_activation);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.memory.stats().objects, before - 1);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(runtime.resources().active_session().is_none());
}
