#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    frame::{ExecutionStack, FrameStack, NativeEntryState},
    gc::retention_probe::{heap_owner, measure, replace_cell},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        storage::{NativePayload, NativeStorage},
    },
    resource::HeapCharge,
    session::owned::OwnedExecution,
    value::{EphemeralValueId, Value},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::BytecodeInstruction,
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InstructionOrigin,
        RootSlotLayout,
    },
    program::ModuleRef,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_contract::ids::FunctionRef;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub(in crate::frame) fn module(
    runtime: &mut Runtime,
    aliases: usize,
    scalar: bool,
) -> ExecutableModule {
    let types = vec![
        if scalar {
            ValueType::I32
        } else {
            ValueType::HeapObject
        };
        aliases
    ];
    let function = BytecodeFunction {
        id: FunctionRef::new(0),
        name: "hold".into(),
        slot_count: aliases as u32,
        metadata: FunctionMetadata {
            slots: types.clone(),
            roots: RootSlotLayout::from_types(&types),
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
    };
    let program = ProgramFixture {
        root: ModuleRef::new(0),
        exports: Default::default(),
        modules: vec![BytecodeModule {
            types: vec![ValueType::Unit, ValueType::HeapObject, ValueType::I32],
            function_table: vec![FunctionRecord {
                id: function.id,
                identity: function.identity.clone(),
                name: function.name.clone(),
                params: vec![],
                return_type: function.metadata.return_type,
                effects: Default::default(),
            }],
            functions: vec![function],
            ..Default::default()
        }],
    }
    .build(runtime.definition_context());
    let program =
        DefinitionMetadata::checked(runtime.host().definitions(), program, &Default::default())
            .unwrap();
    let program = load_bytecode(runtime, "parking", program).unwrap();
    runtime.select_program(&program).unwrap()
}

fn graph(runtime: &Runtime, size: usize, cyclic: bool) -> Value {
    let cell = runtime
        .gc()
        .alloc_cell(ValueType::Unit, Value::Unit)
        .unwrap();
    let mut value = Value::Cell(cell);
    for _ in 1..size {
        value = runtime.gc().alloc_tuple(vec![value]).unwrap();
    }
    if cyclic {
        replace_cell(runtime.gc(), cell, value);
    }
    value
}

pub(in crate::frame) fn set_frame(runtime: &Runtime, stack: &FrameStack, value: Value) {
    let frame = stack.current(runtime).unwrap();
    let mut windows = runtime.resources().frame_values.borrow_mut();
    let range = windows.ranges(frame.slots).unwrap().managed;
    windows.values[range].fill(value);
}

fn fixture(
    runtime: &mut Runtime,
    module: &ExecutableModule,
    shape: &str,
    depth: usize,
    aliases: usize,
) -> OwnedExecution {
    let owner = runtime
        .start_owned_execution(module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let mut stack_activation = runtime.resume_owned_execution(&owner).unwrap();
    stack_activation
        .with(|runtime, stack| {
            let shared = if shape == "scalar" {
                Value::Unit
            } else {
                graph(runtime, 8, shape == "cyclic")
            };
            for index in 0..depth {
                if index != 0 {
                    stack
                        .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                        .unwrap();
                }
                let value = if shape == "disjoint" {
                    graph(runtime, 8, false)
                } else {
                    shared
                };
                if shape != "scalar" {
                    set_frame(runtime, stack, value);
                }
                let frame = stack.current(runtime).unwrap();
                let windows = runtime.resources().frame_values.borrow();
                let ranges = windows.ranges(frame.slots).unwrap();
                assert_eq!(
                    ranges.managed.len(),
                    if shape == "scalar" { 0 } else { aliases }
                );
            }
            Ok(())
        })
        .unwrap();
    transition(stack_activation);
    owner
}

fn transition(stack: ExecutionStack<'_>) {
    let mut activation = Some(stack);
    assert!(ExecutionStack::try_park_ready(&mut activation).unwrap());
    assert!(activation.is_none());
}

#[test]
fn declined_parking_retains_activation_for_retry_and_errors_retain_trace() {
    let mut runtime = Runtime::default();
    let module = module(&mut runtime, 1, false);
    let owner = runtime
        .start_owned_execution(&module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            set_frame(runtime, stack, Value::RuntimeEphemeral(EphemeralValueId(1)));
            Ok(())
        })
        .unwrap();
    assert!(!ExecutionStack::try_park_ready(&mut activation).unwrap());
    assert!(!owner.is_ready());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, _| {
            assert_eq!(runtime.resources().counters().current_call_depth, 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        activation
            .as_mut()
            .unwrap()
            .with(|runtime, stack| Ok(stack.current(runtime)?.instruction_offset()))
            .unwrap(),
        0
    );
    assert!(
        activation
            .as_mut()
            .unwrap()
            .drive(|_, stack| FrameStack::try_park_ready(stack, &Runtime::default()))
            .is_err()
    );
    assert!(activation.is_some());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, _| {
            assert_eq!(runtime.capture_error_trace().frames.len(), 1);
            Ok(())
        })
        .unwrap();
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            set_frame(runtime, stack, Value::Unit);
            Ok(())
        })
        .unwrap();
    assert!(ExecutionStack::park_waiting(&mut activation).is_err());
    assert!(
        activation.is_some(),
        "strict wait without pending must preserve diagnostics"
    );
    assert!(ExecutionStack::try_park_ready(&mut activation).unwrap());
    assert!(activation.is_none());
    assert!(owner.is_ready());
    assert!(ExecutionStack::try_park_ready(&mut activation).is_err());
    drop(activation);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    let mut stack_activation = runtime.resume_owned_execution(&owner).unwrap();
    stack_activation
        .with(|runtime, stack| {
            assert_eq!(
                stack
                    .current_mut(runtime)
                    .unwrap()
                    .next_instruction()
                    .unwrap(),
                BytecodeInstruction::Return(None)
            );

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    runtime.finish_owned_execution(&owner).unwrap();
}

#[test]
fn parking_rechecks_alias_mutation_and_preserves_frame_error_order() {
    let mut runtime = Runtime::default();
    let module = module(&mut runtime, 1, false);
    let owner = runtime
        .start_owned_execution(&module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let cell = runtime
        .gc()
        .alloc_cell(ValueType::Unit, Value::Unit)
        .unwrap();
    let mut stack = runtime.resume_owned_execution(&owner).unwrap();
    stack
        .with(|runtime, stack| {
            set_frame(runtime, stack, Value::Cell(cell));
            stack.push(runtime, module.slot(), FunctionRef::new(0), &[], None)?;
            set_frame(runtime, stack, Value::Cell(cell));
            Ok(())
        })
        .unwrap();
    transition(stack);
    replace_cell(
        runtime.gc(),
        cell,
        Value::RuntimeEphemeral(EphemeralValueId(1)),
    );
    let probe = heap_owner(runtime.gc());
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());

    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            stack.current_mut(runtime)?.native_entry = NativeEntryState::Running;
            Ok(())
        })
        .unwrap();
    let (result, counts) = measure(probe, || ExecutionStack::try_park_ready(&mut activation));
    assert_eq!(result.unwrap_err().kind(), RuntimeErrorKind::ScriptTrap);
    assert_eq!(
        counts.frames, 1,
        "earlier graph error precedes later non-parkability"
    );
    assert!(activation.is_some());
    assert!(!owner.is_ready());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, _| {
            assert!(!runtime.is_quarantined());
            assert_eq!(runtime.capture_error_trace().frames.len(), 2);
            Ok(())
        })
        .unwrap();

    drop(activation);
    runtime.finish_owned_execution(&owner).unwrap();
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[derive(Debug)]
struct TraceFailure(Arc<AtomicBool>);

impl NativePayload for TraceFailure {
    fn trace<'a>(&'a self, _: &mut dyn FnMut(&'a Value)) {
        assert!(!self.0.load(Ordering::Relaxed), "parking trace failure");
    }

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::default())
    }
}

#[test]
fn parking_trace_panic_is_an_error_not_a_declined_transition() {
    let mut runtime = Runtime::default();
    let module = module(&mut runtime, 1, false);
    let owner = runtime
        .start_owned_execution(&module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let fail = Arc::new(AtomicBool::new(false));
    let object = NativeStorage::payload::<TraceFailure>()
        .prepare_payload(
            runtime.gc(),
            &Ty::Builtin(BuiltinType::Unit),
            TraceFailure(fail.clone()),
            &module,
        )
        .unwrap();
    let value = Value::GcHandle(runtime.gc().alloc_native(object).unwrap());
    let mut stack = runtime.resume_owned_execution(&owner).unwrap();
    stack
        .with(|runtime, stack| {
            set_frame(runtime, stack, value);
            Ok(())
        })
        .unwrap();
    transition(stack);
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());

    fail.store(true, Ordering::Relaxed);
    let error = ExecutionStack::try_park_ready(&mut activation).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, _| {
            assert!(runtime.is_quarantined());
            Ok(())
        })
        .unwrap();
    assert!(activation.is_some());
    assert!(!owner.is_ready());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, _| {
            assert_eq!(runtime.capture_error_trace().frames.len(), 1);
            Ok(())
        })
        .unwrap();

    drop(activation);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    let _ = runtime.finish_owned_execution(&owner);
    assert_eq!(
        runtime.resources().frame_values.borrow().active_windows(),
        0
    );
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn parking_transition_probe() {
    for shape in ["shared", "disjoint", "cyclic", "scalar"] {
        for (depth, aliases) in [(1, 1), (4, 1), (4, 3)] {
            let mut runtime = Runtime::default();
            let module = module(&mut runtime, aliases, shape == "scalar");
            let owner = fixture(&mut runtime, &module, shape, depth, aliases);
            let probe = heap_owner(runtime.gc());
            let stack = runtime.resume_owned_execution(&owner).unwrap();
            let (_, counts) = measure(probe, || transition(stack));
            println!(
                "RL02 {shape} D={depth} A={aliases}: proofs={} frames={} walks={} roots={} objects={}/{} edges={}/{}",
                counts.proofs,
                counts.frames,
                counts.walks,
                counts.roots,
                counts.objects,
                counts.distinct_objects.len(),
                counts.edges,
                counts.distinct_edges.len()
            );
            let unique = if shape == "scalar" {
                0
            } else if shape == "disjoint" {
                depth * 8
            } else {
                8
            };
            assert_eq!(counts.distinct_objects.len(), unique);
            assert_eq!(counts.proofs, 1);
            assert_eq!(counts.frames, depth);
            assert_eq!(counts.walks, 1);
            assert_eq!(
                counts.roots,
                if shape == "scalar" {
                    0
                } else {
                    depth * aliases
                }
            );
            assert_eq!(counts.objects, unique);
            assert_eq!(counts.edges, unique);
            assert_eq!(counts.distinct_edges.len(), unique);
            if std::env::var_os("KAGARI_PARK_TIMING").is_some() {
                let mut samples = Vec::new();
                for sample in 0..6 {
                    let mut elapsed = 0;
                    for _ in 0..1000 {
                        let stack = runtime.resume_owned_execution(&owner).unwrap();
                        let start = Instant::now();
                        transition(black_box(stack));
                        elapsed += start.elapsed().as_nanos();
                    }
                    if sample != 0 {
                        samples.push(elapsed);
                    }
                }
                println!("RL02 TIME {shape} D={depth} A={aliases}: ns/1000={samples:?}");
            }
            runtime.finish_owned_execution(&owner).unwrap();
            assert_eq!(runtime.resources().counters().current_call_depth, 0);
        }
    }
}
