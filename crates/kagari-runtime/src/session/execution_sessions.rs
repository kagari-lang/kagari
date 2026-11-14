#[cfg(test)]
use crate::test_support::{bytecode::load_bytecode, update_programs::load_empty as load};

use crate::test_support as support;
use crate::test_support::native::program::ProgramFixture;

use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution::roots::ExecutionRoots,
    frame::types::compatibility::TypeView,
    host::{HostCallContext, HostPathDescriptorId},
    module::managed::source::RevisionSource,
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::FunctionDecl,
        host::callback, types::Type,
    },
    reload::ReloadValidationError,
    session::{DeterministicInputs, ExecutionEntry, ExecutionPhase},
    value::{carriers::RootedValue, view::ValueView},
    value_check::scoped,
};
use kagari_bytecode::{instruction::ModuleSlot, program::ModuleRef};
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
    let mut outer = runtime
        .admit_execution(
            &module,
            options.clone(),
            ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();
    let first = runtime.next_execution_random_u64().unwrap();
    let mut conflicting = options.clone();
    conflicting.inputs = DeterministicInputs {
        unix_time_millis: 999,
        random_seed: 999,
    };
    let mut nested = runtime
        .admit_execution(
            &module,
            conflicting,
            ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();
    assert_eq!(runtime.execution_time_millis().unwrap(), 123_456);
    let second = runtime.next_execution_random_u64().unwrap();
    outer.retire(&mut runtime);
    let third = runtime.next_execution_random_u64().unwrap();
    nested.retire(&mut runtime);

    let replay = runtime.begin_execution(&module, options.clone()).unwrap();
    assert_eq!(replay.runtime().execution_time_millis().unwrap(), 123_456);
    assert_eq!(replay.runtime().next_execution_random_u64().unwrap(), first);
    assert_eq!(
        replay.runtime().next_execution_random_u64().unwrap(),
        second
    );
    assert_eq!(replay.runtime().next_execution_random_u64().unwrap(), third);
    drop(replay);

    options.inputs.random_seed = 43;
    let changed = runtime.begin_execution(&module, options).unwrap();
    assert_ne!(
        changed.runtime().next_execution_random_u64().unwrap(),
        first
    );
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
    let mut outer = runtime
        .admit_execution(
            &module,
            options.clone(),
            ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();
    runtime.resources().poll_execution().unwrap();
    let escalation = options.clone();

    let mut nested = runtime
        .admit_execution(
            &module,
            escalation,
            ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();

    runtime.resources().poll_execution().unwrap();
    token.cancel();
    let error = runtime.resources().poll_execution().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);

    outer.retire(&mut runtime);
    assert!(runtime.resources().poll_execution().is_err());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(module.key())
            .active_calls,
        1
    );
    nested.retire(&mut runtime);
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
    _next.runtime().resources().poll_execution().unwrap();
    _next.runtime().resources().poll_execution().unwrap();
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
    let mut session = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    let inherited = session.runtime().execution_options();
    assert!(
        session
            .runtime_mut()
            .begin_execution(&other, inherited.clone())
            .is_err()
    );
    assert!(
        session
            .runtime_mut()
            .begin_execution(&foreign_module, inherited)
            .is_err()
    );
    let before = session.runtime().resources().counters();
    let stats = session.runtime().gc_stats();
    std::thread::scope(|threads| {
        let runtime = session.runtime_mut();
        let module = &module;
        threads
            .spawn(move || {
                assert_eq!(
                    runtime
                        .begin_execution(module, Default::default())
                        .err()
                        .unwrap()
                        .kind(),
                    RuntimeErrorKind::ExecutionPhaseViolation
                );
                assert_eq!(
                    runtime.collect_garbage().unwrap_err().kind(),
                    RuntimeErrorKind::ExecutionPhaseViolation
                );
                assert_eq!(
                    runtime
                        .alloc_array(module, Ty::Builtin(BuiltinType::Unit), &[])
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::ExecutionPhaseViolation
                );
            })
            .join()
            .unwrap();
    });
    assert_eq!(session.runtime().resources().counters(), before);
    assert_eq!(session.runtime().gc_stats(), stats);
    session.runtime().resources().poll_execution().unwrap();
    assert_eq!(session.root().key(), module.key());
    assert_eq!(
        session
            .runtime()
            .modules()
            .retention_counts(other.key())
            .active_calls,
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
    let mut session = runtime.begin_execution(&module, options.clone()).unwrap();
    let before_objects = session.runtime().memory.stats().objects;
    let object = session
        .runtime_mut()
        .alloc_array(&module, Ty::Builtin(BuiltinType::I32), &[42i32.into()])
        .unwrap();
    token.cancel();
    // Cancellation detected by a matcher cannot inspect suspended frames while
    // the caller still owns their control/source loans. Admission happens only
    // after both loans end, before any invocation cleanup unwinds those frames.
    let error = {
        let Runtime {
            memory,
            resources,
            host,
            host_borrows,
            ..
        } = session.runtime_mut();
        let id = resources.active_session().unwrap().id;
        let _frames = resources.sessions.frames_mut(id).unwrap();
        let expected = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
        let error = memory
            .scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let actual = object.get(slots.scope())?;
                    Ok(scoped::matches(
                        slots.scope(),
                        actual,
                        TypeView::new(&expected, &module, None),
                        host,
                        host_borrows,
                        resources,
                    ))
                })
            })
            .unwrap()
            .unwrap_err();
        assert!(resources.termination().is_none());
        error
    };
    let error = error.into_runtime(session.runtime());
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
    assert!(error.trace().is_none());
    let error = session.runtime().resources().admit_execution_error(error);
    assert!(error.trace().is_some());
    assert!(Arc::ptr_eq(
        error.trace().unwrap(),
        session
            .runtime()
            .resources()
            .termination()
            .unwrap()
            .trace()
            .unwrap(),
    ));
    assert_eq!(
        session.runtime_mut().gc_safepoint().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(
        session
            .runtime_mut()
            .alloc_array(&module, Ty::Builtin(BuiltinType::I32), &[])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::Cancelled
    );
    let inherited = session.runtime().execution_options();
    assert!(
        session
            .runtime_mut()
            .begin_execution(&module, inherited)
            .is_err()
    );
    assert!(!session.runtime().is_quarantined());
    drop(object);
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
    let mut _next = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    _next.runtime_mut().collect_garbage().unwrap();
    assert_eq!(_next.runtime().memory.stats().objects, before_objects);
    _next.runtime().resources().poll_execution().unwrap();
}

#[test]
fn root_heap_peak_counters_do_not_reuse_a_previous_roots_peak() {
    let mut runtime = Runtime::default();
    let module_program = load(&mut runtime, "main");
    let module = runtime.select_program(&module_program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let installed_units = baseline.objects as u128 + baseline.provider_units;
    assert!(installed_units > 0);
    let previous_lifetime_peak = runtime.resources().counters().peak_heap_units;
    assert_eq!(
        runtime.resources().counters().current_heap_units,
        installed_units
    );
    for depth in [2, 1] {
        let mut session = runtime
            .begin_execution(&module, runtime.execution_options())
            .unwrap();
        assert_eq!(session.counters().current_heap_units, installed_units);
        assert_eq!(session.counters().peak_heap_units, installed_units);
        let expected_peak = installed_units + depth as u128 + 1;
        let array = session
            .runtime_mut()
            .alloc_array(
                &module,
                Ty::Builtin(BuiltinType::Unit),
                &vec![RootedValue::unit(); depth as usize],
            )
            .unwrap();
        assert_eq!(session.counters().current_heap_units, expected_peak);
        assert_eq!(session.counters().peak_heap_units, expected_peak);
        let committed = session.runtime().gc_stats();
        assert_eq!(
            expected_peak,
            committed.objects as u128 + committed.provider_units
        );
        session.runtime_mut().collect_garbage().unwrap();
        {
            let mut cx = ConversionContext::new(session.runtime_mut(), &module).unwrap();
            let value = cx.import(&array).unwrap();
            assert_eq!(cx.array_len(&value).unwrap(), depth as usize);
        }
        drop(array);
        session.runtime_mut().collect_garbage().unwrap();
        assert_eq!(session.runtime().gc_stats().objects, baseline.objects);
        assert_eq!(
            session.runtime().gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(session.counters().current_heap_units, installed_units);
        assert_eq!(session.counters().peak_heap_units, expected_peak);
        drop(session);
    }
    assert_eq!(
        runtime.resources().counters().peak_heap_units,
        previous_lifetime_peak.max(installed_units + 3)
    );
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
            .bind(
                function,
                callback(move |_: &mut HostCallContext<'_>, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(ValueView::unit())
                }),
            )
            .unwrap();
    }
    let declarations = builder.finish().unwrap();
    let program = free_function_program(&declarations.to_declaration());
    declarations.install(&mut runtime).unwrap();
    let module_program = load_bytecode(&mut runtime, "main", program).unwrap();
    let module = runtime.select_program(&module_program).unwrap();
    let mut options = runtime.execution_options();
    options.phase = ExecutionPhase::CandidateInitialization;
    let mut outer = runtime
        .admit_execution(
            &module,
            options,
            ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();
    let mut nested_options = runtime.execution_options();
    nested_options.phase = ExecutionPhase::Ordinary;
    let mut nested = runtime
        .admit_execution(
            &module,
            nested_options,
            ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();
    outer.retire(&mut runtime);
    assert_eq!(
        runtime.execution_options().phase,
        ExecutionPhase::CandidateInitialization
    );
    let _ = invoke(&mut runtime, &module, 0, &[]).unwrap();
    let _ = invoke(&mut runtime, &module, 1, &[]).unwrap();
    let before = runtime.resources().counters();
    for slot in 2..5 {
        assert_eq!(
            invoke(&mut runtime, &module, slot, &[]).unwrap_err().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
    }
    // Reject before descriptor lookup, argument traversal, adapters or dirty records.
    let missing = HostPathDescriptorId::new(99);
    assert_eq!(
        runtime
            .read_host_path(&RootedValue::unit(), missing, &[])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        runtime
            .set_host_path(&RootedValue::unit(), missing, &[], &RootedValue::unit())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert!(runtime.host_dirty_paths().is_empty());
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    nested.retire(&mut runtime);
    assert_eq!(runtime.execution_options().phase, ExecutionPhase::Ordinary);
    let _ = invoke(&mut runtime, &module, 3, &[]).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn candidate_initialization_cannot_silently_join_an_ordinary_session() {
    let mut runtime = Runtime::default();
    let module_program = load(&mut runtime, "main");
    let module = runtime.select_program(&module_program).unwrap();
    let mut _session = runtime
        .begin_execution(&module, runtime.execution_options())
        .unwrap();
    let mut options = _session.runtime().execution_options();
    options.phase = ExecutionPhase::CandidateInitialization;
    assert_eq!(
        _session
            .runtime_mut()
            .begin_execution(&module, options)
            .err()
            .expect("ordinary session must reject candidate entry")
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        _session.runtime().execution_options().phase,
        ExecutionPhase::Ordinary
    );
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
                callback(move |context, _| {
                    if symbol == "old" {
                        let retained = retained.lock().unwrap();
                        return Ok(context.import(retained.as_ref().unwrap()).unwrap());
                    }
                    let owner = context.runtime().execution_root().unwrap();
                    let inner = context
                        .runtime_mut()
                        .alloc_array(
                            &owner,
                            Ty::Builtin(BuiltinType::I32),
                            &[RootedValue::from(42i32)],
                        )
                        .unwrap();
                    let outer = context
                        .runtime_mut()
                        .alloc_array(
                            &owner,
                            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
                            &[inner],
                        )
                        .unwrap();
                    Ok(context.import(&outer).unwrap())
                }),
            )
            .unwrap();
    }
    let declarations = builder.finish().unwrap();
    let program = free_function_program(&declarations.to_declaration());
    declarations.install(&mut runtime).unwrap();
    let baseline_program = load_bytecode(&mut runtime, "main", program).unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    let old_object = runtime
        .alloc_array(
            &baseline,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(7i32)],
        )
        .unwrap();
    let old_object = runtime
        .alloc_array(
            &baseline,
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            &[old_object],
        )
        .unwrap();
    let root = old_object;
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
    let mut session = runtime.begin_candidate_initialization(&candidate).unwrap();
    assert_eq!(
        invoke(session.runtime_mut(), candidate.module(), 0, &[])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    let fresh = invoke(session.runtime_mut(), candidate.module(), 1, &[]).unwrap();
    session.runtime_mut().collect_garbage().unwrap();
    drop(session);
    // A new session for the same exact staged program may use its retained data;
    // allocation provenance does not depend on the retired SessionId or Arc.
    {
        let mut session = runtime.begin_candidate_initialization(&candidate).unwrap();
        let mut cx = ConversionContext::new(session.runtime_mut(), candidate.module()).unwrap();
        let fresh = cx.import(&fresh).unwrap();
        assert_eq!(cx.array_len(&fresh).unwrap(), 1);
        let inner = cx.array_element(&fresh, 0usize).unwrap().unwrap();
        assert_eq!(
            cx.array_element(&inner, 0usize).unwrap().unwrap().as_i32(),
            Some(42)
        );
        cx.array_set(&inner, 0usize, &ValueView::from(43i32))
            .unwrap();
        assert_eq!(
            cx.array_element(&inner, 0usize).unwrap().unwrap().as_i32(),
            Some(43)
        );
        assert_eq!(
            cx.import(&root).unwrap_err().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
    }
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&baseline_program).unwrap();
    let old = invoke(&mut runtime, &current, 0, &[]).unwrap();
    let mut cx = ConversionContext::new(&mut runtime, &current).unwrap();
    let old = cx.import(&old).unwrap();
    let inner = cx.array_element(&old, 0usize).unwrap().unwrap();
    assert_eq!(
        cx.array_element(&inner, 0usize).unwrap().unwrap().as_i32(),
        Some(7)
    );
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
    let mut session = runtime.begin_candidate_initialization(&candidate).unwrap();
    for external in [&old, &other, candidate.module()] {
        assert!(
            session
                .runtime_mut()
                .module_instance_snapshot(external)
                .is_none()
        );
        assert_eq!(
            session
                .runtime_mut()
                .read_module_slot(external, ModuleSlot::new(0))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
    }
    let empty = {
        let mut cx = ConversionContext::new(session.runtime_mut(), candidate.module()).unwrap();
        let value = cx.allocate_tuple(&[]).unwrap();
        cx.retain(&value).unwrap()
    };
    assert_eq!(
        session
            .runtime_mut()
            .write_module_slot(candidate.module(), ModuleSlot::new(0), &empty)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert!(!session.runtime_mut().is_quarantined());
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
    let mut session = runtime.begin_candidate_initialization(&candidate).unwrap();
    let local = {
        let mut cx = ConversionContext::new(session.runtime_mut(), candidate.module()).unwrap();
        let value = cx.allocate_tuple(&[]).unwrap();
        cx.retain(&value).unwrap()
    };
    drop(session);
    assert_eq!(
        runtime
            .write_module_slot(candidate.module(), ModuleSlot::new(0), &local)
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
        let mut outer = runtime.begin_execution(&baseline, options).unwrap();
        if mode == 0 {
            token.cancel();
        }
        let expected = RuntimeErrorKind::Cancelled;
        if mode == 0 {
            assert!(
                outer
                    .runtime_mut()
                    .begin_candidate_initialization(&candidate)
                    .is_err()
            );
        } else {
            let session = outer
                .runtime_mut()
                .begin_candidate_initialization(&candidate)
                .unwrap();
            token.cancel();
            drop(session);
        }
        assert_eq!(candidate.initialization_error().unwrap().kind(), expected);
        assert_eq!(
            outer.runtime().execution_root().unwrap().key(),
            baseline.key()
        );
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
