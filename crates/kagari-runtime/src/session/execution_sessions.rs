mod publication;

use crate::test_support as support;
use crate::test_support::native::program::ProgramFixture;

use crate::{
    Runtime,
    error::RuntimeErrorKind,
    gc::roots::RootedValue,
    host::{HostCallContext, HostPathDescriptorId},
    module::Program,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    reload::ReloadValidationError,
    session::{DeterministicInputs, ExecutionPhase},
    value::Value,
};
use kagari_bytecode::{instruction::ModuleSlot, module::BytecodeModule, program::ModuleRef};
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects},
    scalar::BuiltinType,
    ty::Ty,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use support::native::{invoke::invoke, program::free_function_program};

fn load(runtime: &mut Runtime, name: &str) -> Program {
    let context = DefinitionContext::new().unwrap();
    runtime
        .load_program(
            name,
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule {
                    ..Default::default()
                }],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap()
}

#[test]
fn deterministic_inputs_and_random_stream_belong_to_the_root_session() {
    let mut runtime = Runtime::default();
    let module_program = load(&mut runtime, "inputs");
    let module = runtime.select_program(&module_program).unwrap();
    assert!(runtime.execution_time_millis().is_err());
    assert!(runtime.next_execution_random_u64().is_err());
    let mut options = runtime.execution_options();
    options.inputs = DeterministicInputs {
        unix_time_millis: 123_456,
        random_seed: 42,
    };
    let outer = runtime.begin_execution(&module, options.clone()).unwrap();
    let first = runtime.next_execution_random_u64().unwrap();
    let mut conflicting = options.clone();
    conflicting.inputs = DeterministicInputs {
        unix_time_millis: 999,
        random_seed: 999,
    };
    let nested = runtime.begin_execution(&module, conflicting).unwrap();
    assert_eq!(runtime.execution_time_millis().unwrap(), 123_456);
    let second = runtime.next_execution_random_u64().unwrap();
    drop(outer);
    let third = runtime.next_execution_random_u64().unwrap();
    drop(nested);

    let replay = runtime.begin_execution(&module, options.clone()).unwrap();
    assert_eq!(runtime.execution_time_millis().unwrap(), 123_456);
    assert_eq!(runtime.next_execution_random_u64().unwrap(), first);
    assert_eq!(runtime.next_execution_random_u64().unwrap(), second);
    assert_eq!(runtime.next_execution_random_u64().unwrap(), third);
    drop(replay);

    options.inputs.random_seed = 43;
    let changed = runtime.begin_execution(&module, options).unwrap();
    assert_ne!(runtime.next_execution_random_u64().unwrap(), first);
    drop(changed);
    assert!(runtime.next_execution_random_u64().is_err());
}

#[test]
fn nested_scopes_share_cancellation_and_lifetime_even_if_outer_drops_first() {
    let mut runtime = Runtime::default();
    let module_program = load(&mut runtime, "main");
    let module = runtime.select_program(&module_program).unwrap();
    let options = runtime.execution_options();

    let token = options.cancellation.clone();
    let outer = runtime.begin_execution(&module, options.clone()).unwrap();
    runtime.resources().poll_execution().unwrap();
    let escalation = options.clone();

    let nested = runtime.begin_execution(&module, escalation).unwrap();

    runtime.resources().poll_execution().unwrap();
    token.cancel();
    let error = runtime.resources().poll_execution().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);

    drop(outer);
    assert!(runtime.resources().poll_execution().is_err());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        1
    );
    drop(nested);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
    assert!(runtime.resources().termination().is_none());
    let _next = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    runtime.resources().poll_execution().unwrap();
    runtime.resources().poll_execution().unwrap();
}

#[test]
fn nested_execution_rejects_an_unpinned_program_or_foreign_runtime() {
    let mut runtime = Runtime::default();
    let module_program = load(&mut runtime, "main");
    let module = runtime.select_program(&module_program).unwrap();
    let other_program = load(&mut runtime, "other");
    let other = runtime.select_program(&other_program).unwrap();
    let mut foreign = Runtime::default();
    let foreign_module_program = load(&mut foreign, "main");
    let foreign_module = foreign.select_program(&foreign_module_program).unwrap();
    let session = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    assert!(
        runtime
            .begin_execution(&other, runtime.execution_options())
            .is_err()
    );
    assert!(
        runtime
            .begin_execution(&foreign_module, runtime.execution_options())
            .is_err()
    );
    assert_eq!(session.root().key(), module.key());
    assert_eq!(
        runtime.modules().retention_counts(other.key()).active_calls,
        0
    );
}

#[test]
fn cancellation_is_sticky_until_all_scopes_exit_and_next_root_can_run() {
    let mut runtime = Runtime::default();
    let module_program = load(&mut runtime, "main");
    let module = runtime.select_program(&module_program).unwrap();
    let token = CancellationToken::default();
    let mut options = runtime.execution_options();
    options.cancellation = token.clone();
    let session = runtime.begin_execution(&module, options.clone()).unwrap();
    let object = runtime
        .alloc_array(&module, Ty::Builtin(BuiltinType::I32), vec![Value::I32(42)])
        .unwrap();
    token.cancel();
    assert_eq!(
        runtime.gc_safepoint().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        runtime
            .alloc_array(&module, Ty::Builtin(BuiltinType::I32), vec![])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::Cancelled
    );
    assert!(
        runtime
            .begin_execution(&module, runtime.execution_options())
            .is_err()
    );
    assert!(!runtime.is_quarantined());
    drop(session);
    assert!(runtime.resources().termination().is_none());
    assert!(runtime.begin_execution(&module, options).is_err());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
    let _next = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().array_len(object).is_none());
    runtime.resources().poll_execution().unwrap();
}

#[test]
fn root_heap_peak_counters_do_not_reuse_a_previous_roots_peak() {
    let mut runtime = Runtime::default();
    let module_program = load(&mut runtime, "main");
    let module = runtime.select_program(&module_program).unwrap();
    for depth in [2, 1] {
        let session = runtime
            .begin_execution(&module, runtime.execution_options())
            .unwrap();
        let array = runtime
            .alloc_array(
                &module,
                Ty::Builtin(BuiltinType::Unit),
                vec![Value::Unit; depth as usize],
            )
            .unwrap();
        assert_eq!(session.counters().peak_heap_units, depth as usize + 1);
        runtime.collect_garbage().unwrap();
        assert!(runtime.gc().array_len(array).is_none());
        drop(session);
    }
    assert_eq!(runtime.resources().counters().peak_heap_units, 3);
}

#[test]
fn candidate_effect_limits_survive_nested_entries_and_release_with_the_session() {
    let mut builder = ModuleBuilder::new("sessions", &Default::default()).unwrap();

    let mut runtime = Runtime::default();

    let calls = Arc::new(AtomicUsize::new(0));
    for (symbol, effects) in [
        (
            "pure",
            HostFunctionEffects {
                may_allocate: true,
                may_trap: true,
                ..Default::default()
            },
        ),
        (
            "configuration",
            HostFunctionEffects {
                may_read_immutable_configuration: true,
                ..Default::default()
            },
        ),
        (
            "service",
            HostFunctionEffects {
                may_call_host_services: true,
                ..Default::default()
            },
        ),
        (
            "mutation",
            HostFunctionEffects {
                may_mutate_host_state: true,
                ..Default::default()
            },
        ),
        (
            "suspend",
            HostFunctionEffects {
                may_suspend: true,
                ..Default::default()
            },
        ),
    ] {
        let function = builder
            .define_function(FunctionDecl::new(symbol).host_policy(HostCallPolicy {
                passing: vec![],
                effects,
            }))
            .unwrap();
        let calls = calls.clone();
        builder
            .bind(function, move |_: &HostCallContext<'_>, _: &[Value]| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Value::Unit)
            })
            .unwrap();
    }
    let declarations = builder.finish().unwrap();
    let program = free_function_program(&declarations.to_declaration());
    declarations.install(&mut runtime).unwrap();
    let module_program = runtime.load_program("main", program).unwrap();
    let module = runtime.select_program(&module_program).unwrap();
    let mut options = runtime.execution_options();
    options.phase = ExecutionPhase::CandidateInitialization;
    let outer = runtime.begin_execution(&module, options).unwrap();
    let mut nested_options = runtime.execution_options();
    nested_options.phase = ExecutionPhase::Ordinary;
    let nested = runtime.begin_execution(&module, nested_options).unwrap();
    drop(outer);
    assert_eq!(
        runtime.execution_options().phase,
        ExecutionPhase::CandidateInitialization
    );
    invoke(&runtime, &module, 0, &[]).unwrap();
    invoke(&runtime, &module, 1, &[]).unwrap();
    let before = runtime.resources().counters();
    for slot in 2..5 {
        assert_eq!(
            invoke(&runtime, &module, slot, &[]).unwrap_err().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
    }
    // Reject before descriptor lookup, argument traversal, adapters or dirty records.
    let missing = HostPathDescriptorId::new(99);
    assert_eq!(
        runtime
            .read_host_path(&Value::Unit, missing, vec![])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        runtime
            .set_host_path(&Value::Unit, missing, vec![], Value::Unit)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert!(runtime.host_dirty_paths().is_empty());
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    drop(nested);
    assert_eq!(runtime.execution_options().phase, ExecutionPhase::Ordinary);
    invoke(&runtime, &module, 3, &[]).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn candidate_initialization_cannot_silently_join_an_ordinary_session() {
    let mut runtime = Runtime::default();
    let module_program = load(&mut runtime, "main");
    let module = runtime.select_program(&module_program).unwrap();
    let _session = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    let mut options = runtime.execution_options();
    options.phase = ExecutionPhase::CandidateInitialization;
    assert_eq!(
        runtime
            .begin_execution(&module, options)
            .err()
            .expect("ordinary session must reject candidate entry")
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(runtime.execution_options().phase, ExecutionPhase::Ordinary);
}

#[test]
fn candidate_host_results_reject_nested_old_objects_but_accept_candidate_allocations() {
    let mut runtime = Runtime::default();
    let retained: Arc<Mutex<Option<RootedValue>>> = Arc::new(Mutex::new(None));
    let mut builder = ModuleBuilder::new("sessions", &Default::default()).unwrap();
    for symbol in ["old", "fresh"] {
        let array = Type::from_semantic(
            Ty::Array(Box::new(Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))))),
            &DefinitionContext::new().unwrap().snapshot(),
        )
        .unwrap();
        let function =
            builder
                .define_function(FunctionDecl::new(symbol).returns(array).host_policy(
                    HostCallPolicy {
                        passing: vec![],
                        effects: HostFunctionEffects {
                            may_allocate: true,
                            ..Default::default()
                        },
                    },
                ))
                .unwrap();
        let retained = retained.clone();
        builder
            .bind(
                function,
                move |context: &HostCallContext<'_>, _: &[Value]| {
                    if symbol == "old" {
                        let retained = retained.lock().unwrap();
                        return Ok(retained
                            .as_ref()
                            .unwrap()
                            .value(context.runtime().gc())
                            .unwrap());
                    }
                    let owner = context.runtime().execution_root().unwrap();
                    let inner = Value::Array(
                        context
                            .runtime()
                            .alloc_array(
                                &owner,
                                Ty::Builtin(BuiltinType::I32),
                                vec![Value::I32(42)],
                            )
                            .unwrap(),
                    );
                    Ok(Value::Array(
                        context
                            .runtime()
                            .alloc_array(
                                &owner,
                                Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
                                vec![inner],
                            )
                            .unwrap(),
                    ))
                },
            )
            .unwrap();
    }
    let declarations = builder.finish().unwrap();
    let program = free_function_program(&declarations.to_declaration());
    declarations.install(&mut runtime).unwrap();
    let baseline_program = runtime.load_program("main", program).unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    let old_object = runtime
        .alloc_array(
            &baseline,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(7)],
        )
        .unwrap();
    let old_object = runtime
        .alloc_array(
            &baseline,
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            vec![Value::Array(old_object)],
        )
        .unwrap();
    let root = runtime.root_value(Value::Array(old_object)).unwrap();
    *retained.lock().unwrap() = Some(root.clone());
    let candidate = runtime
        .stage_reload_program(&baseline_program, None, {
            let (definitions, module) = baseline
                .to_unverified(&Default::default())
                .unwrap()
                .into_parts();
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module],
            }
            .metadata(definitions, &Default::default())
            .unwrap()
        })
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    assert_eq!(
        invoke(&runtime, candidate.module(), 0, &[])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    let fresh = invoke(&runtime, candidate.module(), 1, &[]).unwrap();
    let fresh = runtime.root_value(fresh).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(
        runtime
            .gc()
            .validate_value(&fresh.value(runtime.gc()).unwrap())
    );
    assert!(
        runtime
            .gc()
            .validate_value(&root.value(runtime.gc()).unwrap())
    );
    drop(session);
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&baseline_program).unwrap();
    assert!(invoke(&runtime, &current, 0, &[]).is_ok());
}

#[test]
fn candidate_module_state_access_is_denied_for_every_program() {
    let mut runtime = Runtime::default();
    let old_program = load(&mut runtime, "main");
    let old = runtime.select_program(&old_program).unwrap();
    let other_program = load(&mut runtime, "other");
    let other = runtime.select_program(&other_program).unwrap();
    let candidate = runtime
        .stage_reload_program(&old_program, None, {
            let (definitions, module) =
                old.to_unverified(&Default::default()).unwrap().into_parts();
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![module],
            }
            .metadata(definitions, &Default::default())
            .unwrap()
        })
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    for external in [&old, &other, candidate.module()] {
        assert!(runtime.module_instance_snapshot(external).is_none());
        assert_eq!(
            runtime
                .read_module_slot(external, ModuleSlot::new(0))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
    }
    let empty = runtime.gc().alloc_tuple(vec![]).unwrap();
    assert_eq!(
        runtime
            .write_module_slot(candidate.module(), ModuleSlot::new(0), empty)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert!(!runtime.is_quarantined());
    drop(session);
    assert_eq!(
        runtime.modules().retention_counts(other.key()).active_calls,
        0
    );
    runtime.publish_staged_reload(candidate).unwrap();
}

#[test]
fn candidate_cannot_publish_static_values_between_initialization_and_publication() {
    let mut runtime = Runtime::default();
    let baseline_program = load(&mut runtime, "main");
    let baseline = runtime.select_program(&baseline_program).unwrap();
    let (definitions, module) = baseline
        .to_unverified(&Default::default())
        .unwrap()
        .into_parts();
    let candidate = runtime
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
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    let local = runtime.gc().alloc_tuple(vec![]).unwrap();
    drop(session);
    assert_eq!(
        runtime
            .write_module_slot(candidate.module(), ModuleSlot::new(0), local)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&baseline_program).unwrap();
    assert!(
        runtime
            .module_instance_snapshot(&current)
            .unwrap()
            .module_slots
            .is_empty()
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn candidate_termination_is_cached_after_the_session_is_dropped() {
    for mode in 0..2 {
        let mut runtime = Runtime::default();
        let baseline_program = load(&mut runtime, "main");
        let baseline = runtime.select_program(&baseline_program).unwrap();
        let candidate = runtime
            .stage_reload_program(&baseline_program, None, {
                let (definitions, module) = baseline
                    .to_unverified(&Default::default())
                    .unwrap()
                    .into_parts();
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![module],
                }
                .metadata(definitions, &Default::default())
                .unwrap()
            })
            .unwrap();
        let mut direct = runtime.execution_options();
        direct.phase = ExecutionPhase::CandidateInitialization;
        assert!(runtime.begin_execution(candidate.module(), direct).is_err());
        let mut options = runtime.execution_options();
        let token = CancellationToken::default();
        options.cancellation = token.clone();
        let outer = runtime.begin_execution(&baseline, options).unwrap();
        if mode == 0 {
            token.cancel();
        }
        let expected = RuntimeErrorKind::Cancelled;
        if mode == 0 {
            assert!(runtime.begin_candidate_initialization(&candidate).is_err());
        } else {
            let session = runtime.begin_candidate_initialization(&candidate).unwrap();
            token.cancel();
            drop(session);
        }
        assert_eq!(candidate.initialization_error().unwrap().kind(), expected);
        assert_eq!(runtime.execution_root().unwrap().key(), baseline.key());
        drop(outer);
        assert!(runtime.begin_candidate_initialization(&candidate).is_err());
        let error = runtime.publish_staged_reload(candidate).unwrap_err();
        assert!(matches!(error, ReloadValidationError::Runtime(error) if error.kind() == expected));
        assert_eq!(
            runtime.select_program(&baseline_program).unwrap().key(),
            baseline.key()
        );
        assert_eq!(runtime.modules().loaded_count(), 1);
        assert!(!runtime.is_quarantined());
    }
}
