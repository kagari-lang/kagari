use kagari_contract::{scalar::BuiltinType, types::Ty};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    host::HostPathDescriptorId,
    module::LoadedModule,
    reload::ReloadValidationError,
    session::{DeterministicInputs, ExecutionPhase},
    value::Value,
};

use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::{cancellation::CancellationToken, collection::CollectionAccess};

fn load(runtime: &mut Runtime, name: &str) -> LoadedModule {
    runtime
        .load_program(
            name,
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap()
}

#[test]
fn deterministic_inputs_and_random_stream_belong_to_the_root_session() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "inputs");
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
    let module = load(&mut runtime, "main");
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
    let module = load(&mut runtime, "main");
    let other = load(&mut runtime, "other");
    let mut foreign = Runtime::default();
    let foreign_module = load(&mut foreign, "main");
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
    let module = load(&mut runtime, "main");
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
    let module = load(&mut runtime, "main");
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
    use kagari_common::host_interface::{
        HostFunctionDeclaration, HostFunctionEffects, value_type::HostValueType,
    };
    use kagari_runtime::{host::HostFunction, session::ExecutionPhase};
    use std::{cell::Cell, rc::Rc};

    let mut runtime = Runtime::default();

    let calls = Rc::new(Cell::new(0));
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
        let mut declaration = HostFunctionDeclaration::new(symbol, vec![], HostValueType::Unit);
        declaration.effects = effects;
        let calls = calls.clone();
        runtime
            .register_host_function(HostFunction::new(declaration, move |_, _| {
                calls.set(calls.get() + 1);
                Ok(Value::Unit)
            }))
            .unwrap();
    }
    let module = load(&mut runtime, "main");
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
    runtime.invoke_host("pure", &[]).unwrap();
    runtime.invoke_host("configuration", &[]).unwrap();
    let before = runtime.resources().counters();
    for symbol in ["service", "mutation", "suspend"] {
        assert_eq!(
            runtime.invoke_host(symbol, &[]).unwrap_err().kind(),
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
    assert_eq!(calls.get(), 2);
    drop(nested);
    assert_eq!(runtime.execution_options().phase, ExecutionPhase::Ordinary);
    runtime.invoke_host("mutation", &[]).unwrap();
    assert_eq!(calls.get(), 3);
}

#[test]
fn candidate_initialization_cannot_silently_join_an_ordinary_session() {
    use kagari_runtime::session::ExecutionPhase;
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
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
    use kagari_common::host_interface::{HostFunctionDeclaration, value_type::HostValueType};
    use kagari_runtime::host::HostFunction;
    let mut runtime = Runtime::default();

    let baseline = load(&mut runtime, "main");
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
            Ty::Array(
                Box::new(Ty::Builtin(BuiltinType::I32)),
                CollectionAccess::Mutable,
            ),
            vec![Value::Array(old_object)],
        )
        .unwrap();
    let root = runtime.root_value(Value::Array(old_object)).unwrap();
    for symbol in ["old", "fresh"] {
        let mut declaration = HostFunctionDeclaration::new(
            symbol,
            vec![],
            HostValueType::Array(
                Box::new(HostValueType::Array(
                    Box::new(HostValueType::I32),
                    CollectionAccess::Mutable,
                )),
                CollectionAccess::Mutable,
            ),
        );
        declaration.effects.may_allocate = true;
        let retained = root.clone();
        let allocation_owner = baseline.clone();
        runtime
            .register_host_function(HostFunction::new(declaration, move |context, _| {
                if symbol == "old" {
                    return Ok(retained.value());
                }
                let owner = context
                    .runtime()
                    .execution_root()
                    .unwrap_or_else(|| allocation_owner.clone());
                let inner = Value::Array(
                    context
                        .runtime()
                        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(42)])
                        .unwrap(),
                );
                Ok(Value::Array(
                    context
                        .runtime()
                        .alloc_array(
                            &owner,
                            Ty::Array(
                                Box::new(Ty::Builtin(BuiltinType::I32)),
                                CollectionAccess::Mutable,
                            ),
                            vec![inner],
                        )
                        .unwrap(),
                ))
            }))
            .unwrap();
    }
    let candidate = runtime
        .stage_reload_program(
            &baseline,
            "main",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![baseline.to_unverified(&Default::default()).unwrap()],
            },
        )
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    assert_eq!(
        runtime.invoke_host("old", &[]).unwrap_err().kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    let fresh = runtime.invoke_host("fresh", &[]).unwrap();
    let fresh = runtime.root_value(fresh).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&fresh.value()));
    assert!(runtime.gc().validate_value(&root.value()));
    drop(session);
    runtime.publish_staged_reload(candidate).unwrap();
    assert!(runtime.invoke_host("old", &[]).is_ok());
}

#[test]
fn candidate_module_state_access_is_limited_to_its_program() {
    let mut runtime = Runtime::default();
    let old = load(&mut runtime, "main");
    let other = load(&mut runtime, "other");
    let candidate = runtime
        .stage_reload_program(
            &old,
            "main",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![old.to_unverified(&Default::default()).unwrap()],
            },
        )
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    for external in [&old, &other] {
        assert!(runtime.module_instance_snapshot(external).is_none());
        assert_eq!(
            runtime.module_instance_mut(external).unwrap_err().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert!(
            runtime
                .modules()
                .instance_snapshot(external.key())
                .is_none()
        );
    }
    assert!(
        runtime
            .module_instance_snapshot(candidate.module())
            .is_some()
    );
    assert!(runtime.module_instance_mut(candidate.module()).is_ok());
    assert!(!runtime.is_quarantined());
    drop(session);
    assert_eq!(
        runtime.modules().retention_counts(other.key()).active_calls,
        0
    );
    runtime.publish_staged_reload(candidate).unwrap();
}

#[test]
fn publication_rechecks_objects_after_the_initialization_session_ends() {
    let mut runtime = Runtime::default();
    let baseline = load(&mut runtime, "main");
    let old_object = runtime
        .alloc_array(
            &baseline,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(7)],
        )
        .unwrap();
    for inject_external in [true, false] {
        let candidate = runtime
            .stage_reload_program(
                &baseline,
                "main",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![baseline.to_unverified(&Default::default()).unwrap()],
                },
            )
            .unwrap();
        let session = runtime.begin_candidate_initialization(&candidate).unwrap();
        let inner = runtime
            .alloc_array(
                candidate.module(),
                Ty::Builtin(BuiltinType::I32),
                vec![Value::I32(42)],
            )
            .unwrap();
        let local = runtime
            .alloc_array(
                candidate.module(),
                Ty::Array(
                    Box::new(Ty::Builtin(BuiltinType::I32)),
                    CollectionAccess::Mutable,
                ),
                vec![Value::Array(inner)],
            )
            .unwrap();
        runtime
            .module_instance_mut(candidate.module())
            .unwrap()
            .module_slots = vec![Value::Array(local)];
        drop(session);
        if inject_external {
            // A low-level driver can still mutate candidate state between phases.
            runtime
                .gc()
                .array_push(local, Value::Array(old_object))
                .unwrap();
            let error = runtime.publish_staged_reload(candidate).unwrap_err();
            assert!(
                matches!(error, ReloadValidationError::Runtime(error) if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
            );
            assert_eq!(
                runtime.modules().latest("main").unwrap().key(),
                baseline.key()
            );
            assert_eq!(runtime.resources().counters().loaded_modules, 1);
        } else {
            let current = runtime.publish_staged_reload(candidate).unwrap();
            assert_eq!(
                runtime
                    .module_instance_snapshot(&current)
                    .unwrap()
                    .module_slots,
                vec![Value::Array(local)]
            );
            runtime.collect_garbage().unwrap();
            assert_eq!(
                runtime.gc().array_snapshot(local).unwrap(),
                vec![Value::Array(inner)]
            );
            assert_eq!(
                runtime.gc().array_snapshot(inner).unwrap(),
                vec![Value::I32(42)]
            );
        }
    }
}

#[test]
fn candidate_termination_is_cached_after_the_session_is_dropped() {
    for mode in 0..2 {
        let mut runtime = Runtime::default();
        let baseline = load(&mut runtime, "main");
        let candidate = runtime
            .stage_reload_program(
                &baseline,
                "main",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![baseline.to_unverified(&Default::default()).unwrap()],
                },
            )
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
            runtime.modules().latest("main").unwrap().key(),
            baseline.key()
        );
        assert_eq!(runtime.resources().counters().loaded_modules, 1);
        assert!(!runtime.is_quarantined());
    }
}
