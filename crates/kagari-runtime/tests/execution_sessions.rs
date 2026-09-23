use kagari_common::cancellation::CancellationToken;
use kagari_ir::bytecode::{BytecodeModule, BytecodeProgram, ModuleRef};
use kagari_runtime::{Runtime, RuntimeErrorKind, value::Value};

fn load(runtime: &mut Runtime, name: &str) -> kagari_runtime::LoadedModule {
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
fn nested_scopes_inherit_permissions_budget_and_lifetime_even_if_outer_drops_first() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let mut options = runtime.execution_options();
    options.resources.max_instruction_steps = Some(2);
    let outer = runtime.begin_execution(&module, options.clone()).unwrap();
    runtime.consume_instruction_step().unwrap();
    let mut escalation = options.clone();
    escalation.resources.max_instruction_steps = None;
    escalation.security.profile.allow_host_calls = true;
    escalation.security.capabilities.host_calls = true;
    let nested = runtime.begin_execution(&module, escalation).unwrap();
    assert!(!runtime.security().allows_host_calls());
    runtime.consume_instruction_step().unwrap();
    let error = runtime.consume_instruction_step().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
    assert_eq!(nested.counters().instruction_steps, 2);
    drop(outer);
    assert!(runtime.consume_instruction_step().is_err());
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
    let next = runtime.begin_execution(&module, options).unwrap();
    runtime.consume_instruction_step().unwrap();
    runtime.consume_instruction_step().unwrap();
    assert_eq!(next.counters().instruction_steps, 2);
    assert_eq!(runtime.resources().counters().instruction_steps, 4);
}

#[test]
fn root_permissions_and_host_policy_are_fixed_until_last_scope_exits() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let session = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    let mut security = runtime.security();
    security.profile.allow_host_calls = true;
    security.capabilities.host_calls = true;
    runtime.set_security_context(security);
    let mut exposure = (*runtime.host_exposure()).clone();
    exposure.allowed_host_functions.push("new.host".into());
    runtime.set_host_exposure_policy(exposure);
    assert!(!runtime.security().allows_host_calls());
    assert!(!runtime.host_exposure().exposes_host_function("new.host"));
    drop(session);
    assert!(runtime.security().allows_host_calls());
    assert!(runtime.host_exposure().exposes_host_function("new.host"));
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
    let object = runtime.alloc_array(vec![Value::I32(42)]).unwrap();
    token.cancel();
    assert_eq!(
        runtime.gc_safepoint().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        runtime.alloc_array(vec![]).unwrap_err().kind(),
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
    let next = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().array_len(object).is_none());
    runtime.consume_instruction_step().unwrap();
    assert_eq!(next.counters().instruction_steps, 1);
}

#[test]
fn each_root_gets_an_allocation_budget_while_live_heap_and_cumulative_counts_persist() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let mut options = runtime.execution_options();
    options.resources.max_allocation_units = Some(2);
    let mut roots = Vec::new();
    for index in 1..=2 {
        let session = runtime.begin_execution(&module, options.clone()).unwrap();
        let array = runtime.alloc_array(vec![Value::I32(index)]).unwrap();
        roots.push(runtime.root_value(Value::Array(array)).unwrap());
        assert_eq!(session.counters().allocation_units, 2);
        assert!(runtime.alloc_array(vec![]).is_err());
        drop(session);
    }
    assert_eq!(runtime.resources().counters().allocation_units, 4);
    assert_eq!(runtime.resources().counters().current_heap_units, 4);
    drop(roots);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 0);
    assert_eq!(runtime.resources().counters().allocation_units, 4);
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
            .alloc_array(vec![Value::Unit; depth as usize])
            .unwrap();
        assert_eq!(session.counters().peak_heap_units, depth as usize + 1);
        runtime.collect_garbage().unwrap();
        assert!(runtime.gc().array_len(array).is_none());
        drop(session);
    }
    assert_eq!(runtime.resources().counters().peak_heap_units, 3);
}

#[test]
fn zero_wall_budget_rejects_before_initialization_or_counter_charges() {
    let mut runtime = Runtime::default();
    let module = load(&mut runtime, "main");
    let mut options = runtime.execution_options();
    options.resources.max_wall_time_ms = Some(0);
    let before = runtime.resources().counters();
    let error = runtime.begin_execution(&module, options).err().unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
    assert!(error.message().contains("wall time"));
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        0
    );
    assert_eq!(
        runtime.module_instance_snapshot(&module).unwrap().state,
        kagari_runtime::ModuleInitializationState::Uninitialized
    );
}

#[test]
fn candidate_effect_limits_survive_nested_entries_and_release_with_the_session() {
    use kagari_common::host_interface::{
        HostFunctionDeclaration, HostFunctionEffects, HostValueType,
    };
    use kagari_runtime::{ExecutionPhase, HostExposurePolicy, host::HostFunction};
    use std::{cell::Cell, rc::Rc};

    let mut runtime = Runtime::default();
    let mut security = runtime.security();
    security.profile.allow_host_calls = true;
    security.capabilities.host_calls = true;
    runtime.set_security_context(security);
    runtime.set_host_exposure_policy(HostExposurePolicy {
        allow_host_functions: true,
        ..Default::default()
    });
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
            RuntimeErrorKind::CapabilityDenied
        );
    }
    // Reject before descriptor lookup, argument traversal, adapters or dirty records.
    let missing = kagari_runtime::HostPathDescriptorId::new(99);
    assert_eq!(
        runtime
            .read_host_path(&Value::Unit, missing, vec![])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(
        runtime
            .set_host_path(&Value::Unit, missing, vec![], Value::Unit)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
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
    use kagari_runtime::ExecutionPhase;
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
        RuntimeErrorKind::CapabilityDenied
    );
    assert_eq!(runtime.execution_options().phase, ExecutionPhase::Ordinary);
}

#[test]
fn candidate_host_results_reject_nested_old_objects_but_accept_candidate_allocations() {
    use kagari_common::host_interface::{HostFunctionDeclaration, HostValueType};
    use kagari_runtime::{HostExposurePolicy, host::HostFunction};
    let mut runtime = Runtime::default();
    let mut security = runtime.security();
    security.profile.allow_host_calls = true;
    security.capabilities.host_calls = true;
    runtime.set_security_context(security);
    runtime.set_host_exposure_policy(HostExposurePolicy {
        allow_host_functions: true,
        ..Default::default()
    });
    let old_object = runtime.alloc_array(vec![Value::I32(7)]).unwrap();
    let root = runtime.root_value(Value::Array(old_object)).unwrap();
    for symbol in ["old", "fresh"] {
        let mut declaration = HostFunctionDeclaration::new(
            symbol,
            vec![],
            HostValueType::Array(Box::new(HostValueType::Array(Box::new(HostValueType::I32)))),
        );
        declaration.effects.may_allocate = true;
        let retained = root.clone();
        runtime
            .register_host_function(HostFunction::new(declaration, move |context, _| {
                let inner = if symbol == "old" {
                    retained.value()
                } else {
                    Value::Array(context.runtime().alloc_array(vec![Value::I32(42)]).unwrap())
                };
                Ok(Value::Array(
                    context.runtime().alloc_array(vec![inner]).unwrap(),
                ))
            }))
            .unwrap();
    }
    let baseline = load(&mut runtime, "main");
    let candidate = runtime
        .stage_reload_program(
            &baseline,
            "main",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![(*baseline.bytecode).clone()],
            },
        )
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    assert_eq!(
        runtime.invoke_host("old", &[]).unwrap_err().kind(),
        RuntimeErrorKind::CapabilityDenied
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
fn candidate_heap_mutations_cannot_modify_preexisting_containers() {
    let mut runtime = Runtime::default();
    let array = runtime.alloc_array(vec![Value::I32(7)]).unwrap();
    let map = runtime
        .alloc_map(vec![(Value::I32(1), Value::I32(7))])
        .unwrap();
    let set = runtime.alloc_set(vec![Value::I32(7)]).unwrap();
    let baseline = load(&mut runtime, "main");
    let candidate = runtime
        .stage_reload_program(
            &baseline,
            "main",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![(*baseline.bytecode).clone()],
            },
        )
        .unwrap();
    let retained = runtime
        .root_value(Value::Tuple(vec![
            Value::Array(array),
            Value::Map(map),
            Value::Set(set),
        ]))
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    let before = runtime.resources().counters();
    let heap = runtime.gc();
    assert!(heap.array_push(array, Value::I32(9)).is_err());
    assert!(heap.array_insert(array, 0, Value::I32(9)).is_err());
    assert!(heap.array_set(array, 0, Value::I32(9)).is_err());
    assert!(heap.array_pop(array).is_err());
    assert!(heap.array_remove(array, 0).is_err());
    assert!(heap.array_clear(array).is_err());
    assert!(heap.map_insert(map, Value::I32(1), Value::I32(9)).is_err());
    assert!(heap.map_remove(map, &Value::I32(1)).is_err());
    assert!(heap.map_clear(map).is_err());
    assert!(heap.set_insert(set, Value::I32(9)).is_err());
    assert!(heap.set_remove(set, &Value::I32(7)).is_err());
    assert!(heap.set_clear(set).is_err());
    assert!(heap.array_snapshot(array).is_none());
    assert!(heap.array_len(array).is_none());
    assert!(heap.array_get(array, 0).is_none());
    assert!(heap.map_snapshot(map).is_none());
    assert!(heap.map_len(map).is_none());
    assert!(heap.set_snapshot(set).is_none());
    assert!(heap.set_len(set).is_none());
    assert_eq!(runtime.resources().counters(), before);
    let local = runtime.alloc_array(vec![Value::I32(1)]).unwrap();
    heap.array_push(local, Value::I32(2)).unwrap();
    assert_eq!(heap.array_len(local), Some(2));
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&retained.value()));
    drop(session);
    assert_eq!(heap.array_snapshot(array).unwrap(), vec![Value::I32(7)]);
    assert_eq!(
        heap.map_snapshot(map).unwrap(),
        vec![(Value::I32(1), Value::I32(7))]
    );
    assert_eq!(heap.set_snapshot(set).unwrap(), vec![Value::I32(7)]);
    drop(candidate);
    runtime.gc().array_push(array, Value::I32(9)).unwrap();
    assert_eq!(runtime.gc().array_len(array), Some(2));
    assert!(!runtime.is_quarantined());
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
                modules: vec![(*old.bytecode).clone()],
            },
        )
        .unwrap();
    // An already owned initialization guard must still record failure when dropped.
    let cleanup = runtime.begin_module_initialization(&other).unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    for external in [&old, &other] {
        assert!(runtime.module_instance_snapshot(external).is_none());
        assert!(runtime.module_instance_mut(external).is_none());
        assert!(
            runtime
                .modules()
                .instance_snapshot(external.key())
                .is_none()
        );
        assert!(runtime.modules().instance_mut(external.key()).is_none());
        assert_eq!(
            runtime
                .fail_module_initialization(external)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::CapabilityDenied
        );
    }
    assert!(runtime.begin_module_initialization(&old).is_err());
    assert!(
        runtime
            .module_instance_snapshot(candidate.module())
            .is_some()
    );
    assert!(runtime.module_instance_mut(candidate.module()).is_some());
    drop(cleanup);
    assert!(!runtime.is_quarantined());
    drop(session);
    assert_eq!(
        runtime.module_instance_snapshot(&old).unwrap().state,
        kagari_runtime::ModuleInitializationState::Uninitialized
    );
    assert_eq!(
        runtime.module_instance_snapshot(&other).unwrap().state,
        kagari_runtime::ModuleInitializationState::Failed
    );
    assert_eq!(
        runtime.modules().retention_counts(other.key()).active_calls,
        0
    );
    runtime.publish_staged_reload(candidate).unwrap();
}

#[test]
fn publication_rechecks_objects_after_the_initialization_session_ends() {
    let mut runtime = Runtime::default();
    let old_object = runtime.alloc_array(vec![Value::I32(7)]).unwrap();
    let baseline = load(&mut runtime, "main");
    for inject_external in [true, false] {
        let candidate = runtime
            .stage_reload_program(
                &baseline,
                "main",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![(*baseline.bytecode).clone()],
                },
            )
            .unwrap();
        let session = runtime.begin_candidate_initialization(&candidate).unwrap();
        let local = runtime.alloc_array(vec![Value::I32(42)]).unwrap();
        runtime
            .module_instance_mut(candidate.module())
            .unwrap()
            .init_result = Some(Value::Array(local));
        drop(session);
        if inject_external {
            // A low-level driver can still mutate candidate state between phases.
            runtime
                .gc()
                .array_push(local, Value::Array(old_object))
                .unwrap();
            let error = runtime.publish_staged_reload(candidate).unwrap_err();
            assert!(
                matches!(error, kagari_runtime::ReloadValidationError::Runtime(error) if error.kind() == RuntimeErrorKind::CapabilityDenied)
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
                    .init_result,
                Some(Value::Array(local))
            );
            runtime.collect_garbage().unwrap();
            assert_eq!(
                runtime.gc().array_snapshot(local).unwrap(),
                vec![Value::I32(42)]
            );
        }
    }
}

#[test]
fn candidate_termination_is_cached_after_the_session_is_dropped() {
    for mode in 0..3 {
        let mut runtime = Runtime::default();
        let baseline = load(&mut runtime, "main");
        let candidate = runtime
            .stage_reload_program(
                &baseline,
                "main",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![(*baseline.bytecode).clone()],
                },
            )
            .unwrap();
        let mut direct = runtime.execution_options();
        direct.phase = kagari_runtime::ExecutionPhase::CandidateInitialization;
        assert!(runtime.begin_execution(candidate.module(), direct).is_err());
        let mut options = runtime.execution_options();
        let token = CancellationToken::default();
        options.cancellation = token.clone();
        if mode == 2 {
            options.resources.max_instruction_steps = Some(0);
        }
        let outer = runtime.begin_execution(&baseline, options).unwrap();
        if mode == 0 {
            token.cancel();
        }
        let expected = if mode == 2 {
            RuntimeErrorKind::ResourceLimitExceeded
        } else {
            RuntimeErrorKind::Cancelled
        };
        if mode == 0 {
            assert!(runtime.begin_candidate_initialization(&candidate).is_err());
        } else {
            let session = runtime.begin_candidate_initialization(&candidate).unwrap();
            if mode == 1 {
                token.cancel();
            } else {
                assert!(runtime.consume_instruction_step().is_err());
            }
            drop(session);
        }
        assert_eq!(candidate.initialization_error().unwrap().kind(), expected);
        assert_eq!(runtime.execution_root().unwrap().key(), baseline.key());
        drop(outer);
        assert!(runtime.begin_candidate_initialization(&candidate).is_err());
        let error = runtime.publish_staged_reload(candidate).unwrap_err();
        assert!(
            matches!(error, kagari_runtime::ReloadValidationError::Runtime(error) if error.kind() == expected)
        );
        assert_eq!(
            runtime.modules().latest("main").unwrap().key(),
            baseline.key()
        );
        assert_eq!(runtime.resources().counters().loaded_modules, 1);
        assert!(!runtime.is_quarantined());
    }
}
