//! Owned parking checks actual frame roots and fresh provider graphs.
mod graph;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    frame::{ExecutionStack, FrameStack, NativeEntryState},
    module::ExecutableModule,
    native::conversion::context::ConversionContext,
    session::owned::OwnedExecution,
    test_support::{bytecode::load_bytecode, native::program::ProgramFixture},
    value::{
        aggregates::tuples::TupleObject,
        carriers::{RootedValue, ScopedValue},
        ephemeral::EphemeralValueId,
        view::ValueView,
    },
};
use graph::{Graphs, Probe};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, Slot},
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InstructionOrigin,
        RootSlotLayout,
    },
    program::ModuleRef,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_contract::ids::FunctionRef;
use std::{
    hint::black_box,
    sync::{Arc, atomic::Ordering},
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

fn set_frame(runtime: &mut Runtime, stack: &FrameStack, value: &ValueView<'_>) {
    let count = stack.current(runtime).unwrap().slot_count;
    for slot in 0..count {
        stack.write_slot(runtime, Slot::new(slot), value).unwrap();
    }
}

fn set_rooted(runtime: &mut Runtime, stack: &FrameStack, value: &RootedValue) {
    let owner = stack.current(runtime).unwrap().loaded().clone();
    let mut source = ConversionContext::new(runtime, &owner).unwrap();
    let value = source.import(value).unwrap();
    set_frame(source.runtime_mut(), stack, &value);
}

fn transient_tuple(runtime: &mut Runtime, ephemeral: bool) -> RootedValue {
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let value = if ephemeral {
                ScopedValue::runtime_ephemeral(EphemeralValueId(1))
            } else {
                ScopedValue::unit()
            };
            let tuple = TupleObject::alloc(cx, &[value])?;
            ScopedValue::managed(tuple).retain(cx)
        })
        .unwrap()
}

fn fixture(
    runtime: &mut Runtime,
    module: &ExecutableModule,
    graphs: &Graphs,
    probe: &Arc<Probe>,
    shape: &str,
    depth: usize,
    aliases: usize,
) -> OwnedExecution {
    let owner = runtime
        .start_owned_execution(module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let mut activation = runtime.resume_owned_execution(&owner).unwrap();
    activation
        .with(|runtime, stack| {
            let shared = matches!(shape, "shared" | "cyclic")
                .then(|| graphs.allocate(runtime, 8, shape == "cyclic", probe));
            for index in 0..depth {
                if index != 0 {
                    stack.push(runtime, module.slot(), FunctionRef::new(0), &[], None)?;
                }
                if shape == "disjoint" {
                    let value = graphs.allocate(runtime, 8, false, probe);
                    set_rooted(runtime, stack, &value);
                } else if let Some(value) = &shared {
                    set_rooted(runtime, stack, value);
                }
                assert_eq!(
                    stack.current(runtime)?.storage_counts(runtime)?.1,
                    if shape == "scalar" { 0 } else { aliases }
                );
            }
            Ok(())
        })
        .unwrap();
    transition(activation);
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
            let value = transient_tuple(runtime, true);
            set_rooted(runtime, stack, &value);
            Ok(())
        })
        .unwrap();
    assert!(!ExecutionStack::try_park_ready(&mut activation).unwrap());
    assert!(!owner.is_ready());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            assert_eq!(runtime.resources().counters().current_call_depth, 1);
            assert_eq!(stack.current(runtime)?.instruction_offset(), 0);
            Ok(())
        })
        .unwrap();
    assert!(
        activation
            .as_mut()
            .unwrap()
            .drive(|_, stack| { FrameStack::try_park_ready(stack, &mut Runtime::default()) })
            .is_err()
    );
    assert!(activation.is_some());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            assert_eq!(runtime.capture_error_trace().frames.len(), 1);
            let value = transient_tuple(runtime, false);
            set_rooted(runtime, stack, &value);
            Ok(())
        })
        .unwrap();
    assert!(ExecutionStack::park_waiting(&mut activation).is_err());
    assert!(
        activation.is_some(),
        "strict wait without pending preserves diagnostics"
    );
    assert!(ExecutionStack::try_park_ready(&mut activation).unwrap());
    assert!(activation.is_none());
    assert!(owner.is_ready());
    assert!(ExecutionStack::try_park_ready(&mut activation).is_err());
    drop(activation);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    let mut resumed = runtime.resume_owned_execution(&owner).unwrap();
    resumed
        .with(|runtime, stack| {
            assert_eq!(
                stack.current_mut(runtime)?.next_instruction().unwrap(),
                BytecodeInstruction::Return(None)
            );
            Ok(())
        })
        .unwrap();
    drop(resumed);
    runtime.finish_owned_execution(&owner).unwrap();
    assert!(!runtime.is_quarantined());
}

#[test]
fn parking_rechecks_alias_mutation_and_preserves_frame_error_order() {
    let mut runtime = Runtime::default();
    let graphs = Graphs::install(&mut runtime);
    let module = module(&mut runtime, 1, false);
    let probe = Arc::new(Probe::default());
    let value = graphs.allocate(&mut runtime, 1, false, &probe);
    let owner = runtime
        .start_owned_execution(&module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let mut stack = runtime.resume_owned_execution(&owner).unwrap();
    stack
        .with(|runtime, stack| {
            set_rooted(runtime, stack, &value);
            stack.push(runtime, module.slot(), FunctionRef::new(0), &[], None)?;
            set_rooted(runtime, stack, &value);
            Ok(())
        })
        .unwrap();
    transition(stack);
    graphs.hide_host_root(&mut runtime, &value);
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            // Model the later control frame's synchronous Native restriction. The
            // earlier frame's newly invalid data must be diagnosed first.
            stack.current_mut(runtime)?.native_entry = NativeEntryState::Running;
            Ok(())
        })
        .unwrap();
    probe.scans.store(0, Ordering::Relaxed);
    let error = ExecutionStack::try_park_ready(&mut activation).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert!(error.to_string().contains("async state retains"));
    assert_eq!(probe.scans.load(Ordering::Relaxed), 1);
    assert!(activation.is_some());
    assert!(!owner.is_ready());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, _| {
            assert!(!runtime.is_quarantined());
            assert_eq!(runtime.capture_error_trace().frames.len(), 2);
            assert_eq!(runtime.resources().counters().current_call_depth, 2);
            Ok(())
        })
        .unwrap();
    drop(activation);
    runtime.finish_owned_execution(&owner).unwrap();
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn parking_trace_panic_is_an_error_not_a_declined_transition() {
    let mut runtime = Runtime::default();
    let graphs = Graphs::install(&mut runtime);
    let module = module(&mut runtime, 1, false);
    let probe = Arc::new(Probe::default());
    let value = graphs.allocate(&mut runtime, 1, false, &probe);
    let owner = runtime
        .start_owned_execution(&module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let mut stack = runtime.resume_owned_execution(&owner).unwrap();
    stack
        .with(|runtime, stack| {
            set_rooted(runtime, stack, &value);
            Ok(())
        })
        .unwrap();
    transition(stack);
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());
    probe.fail.store(true, Ordering::Relaxed);
    let error = ExecutionStack::try_park_ready(&mut activation).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(activation.is_some());
    assert!(!owner.is_ready());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            assert!(runtime.is_quarantined());
            assert_eq!(runtime.capture_error_trace().frames.len(), 1);
            assert_eq!(stack.frames(runtime)?.len(), 1);
            Ok(())
        })
        .unwrap();
    drop(activation);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    let _ = runtime.finish_owned_execution(&owner);
    // Provider failure blocks Runtime services. Cleanup and destruction still
    // retire each payload; an error never authorizes a successful transition.
    drop(value);
    drop(runtime);
    assert_eq!(probe.drops.load(Ordering::Relaxed), 1);
}

#[test]
fn parking_transition_probe() {
    for shape in ["shared", "disjoint", "cyclic", "scalar"] {
        for (depth, aliases) in [(1, 1), (4, 1), (4, 3)] {
            let mut runtime = Runtime::default();
            let graphs = Graphs::install(&mut runtime);
            let module = module(&mut runtime, aliases, shape == "scalar");
            runtime.collect_garbage().unwrap();
            let baseline = runtime.gc_stats();
            let probe = Arc::new(Probe::default());
            let owner = fixture(
                &mut runtime,
                &module,
                &graphs,
                &probe,
                shape,
                depth,
                aliases,
            );
            // Drain the fixture's dropped Host inputs before measuring parking.
            // The parked execution must be the graph's only value owner here.
            runtime.collect_garbage().unwrap();
            assert_eq!(
                runtime.gc_stats().root_registrations,
                baseline.root_registrations
            );
            let stack = runtime.resume_owned_execution(&owner).unwrap();
            probe.scans.store(0, Ordering::Relaxed);
            transition(stack);
            let unique = if shape == "scalar" {
                0
            } else if shape == "disjoint" {
                depth * 8
            } else {
                8
            };
            assert_eq!(
                probe.scans.load(Ordering::Relaxed),
                unique,
                "one real provider scan per distinct data object across the operation"
            );
            assert_eq!(
                runtime.gc_stats().root_registrations,
                baseline.root_registrations
            );
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
                println!("PARK_PROVIDER {shape} D={depth} A={aliases}: ns/1000={samples:?}");
            }
            runtime.finish_owned_execution(&owner).unwrap();
            runtime.collect_garbage().unwrap();
            assert_eq!(runtime.gc_stats().objects, baseline.objects);
            assert_eq!(probe.drops.load(Ordering::Relaxed), unique);
            assert_eq!(runtime.resources().counters().current_call_depth, 0);
            assert!(!runtime.is_quarantined());
        }
    }
}
