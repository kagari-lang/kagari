use super::*;
use crate::module::staging::ManagedProgramError;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime, RuntimeConfig, StagedLoad, StagedReload,
    error::{RuntimeErrorKind, fault::EngineFault},
    execution::roots::{ExecutionRoots, ValueBank, ValueLocation},
    module::{
        InstalledProgram, Module, VerifiedProgram, managed::source::RevisionSource,
        retention::RetentionOrigin,
    },
    test_support::native::program::ProgramFixture,
    value::{carriers::ScopedValue, strings::StringObject},
};

use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_gc::{error::GcError, heap::Phase};
use rstest::rstest;
use std::{
    mem,
    task::{Context, Waker},
    thread,
};

fn program() -> DefinitionMetadata<BytecodeProgram> {
    ProgramFixture {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    }
    .metadata(
        DefinitionContext::new().unwrap().snapshot(),
        &Default::default(),
    )
    .unwrap()
}

fn stage(runtime: &mut Runtime, baseline: &InstalledProgram) -> StagedReload {
    runtime
        .stage_reload_program(baseline, None, program())
        .unwrap()
}

fn stage_initial(runtime: &mut Runtime) -> StagedLoad {
    let code = VerifiedProgram::new(program(), runtime.definition_context()).unwrap();
    runtime
        .stage_load_verified_program("initial", code)
        .unwrap()
}

#[test]
fn staged_initial_load_prepares_token_and_metadata_before_visibility() {
    let mut runtime = Runtime::default();
    let candidate = stage_initial(&mut runtime);
    let token = candidate.publication_token();
    let installation = candidate.installation.clone();
    let key = candidate.program.module().key();
    assert_eq!(
        runtime
            .modules
            .capture_current(&installation)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(
        candidate.program.module().source_snapshot().publication(),
        token
    );
    assert!(runtime.modules.is_staged(candidate.program.module()));
    assert!(runtime.modules.current.borrow().is_empty());
    assert_eq!(runtime.gc_stats().root_registrations, 1);

    let live = runtime.publish_staged_load(candidate).unwrap();
    assert_eq!(runtime.publication_token(&live).unwrap(), token);
    assert_eq!(runtime.select_program(&live).unwrap().key(), key);
    assert_eq!(runtime.source_snapshot(&live).unwrap().publication(), token);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.modules.current.borrow().len(), 1);
    assert_eq!(runtime.gc_stats().root_registrations, 1);
}

#[test]
fn staged_initial_load_abandonment_foreign_owner_and_shutdown_never_publish() {
    let mut runtime = Runtime::default();
    let candidate = stage_initial(&mut runtime);
    let token = candidate.publication_token();
    let module = candidate.program.module().clone();
    let installation = candidate.installation.clone();
    let view = runtime.modules.inner.borrow_mut();
    drop(candidate);
    drop(view);
    assert!(runtime.validate_executable_module(&module).is_err());
    assert!(runtime.modules.capture_current(&installation).is_err());
    runtime.collect_garbage().unwrap();
    assert!(
        !runtime
            .modules
            .inner
            .borrow()
            .records
            .contains_key(&module.key())
    );

    let candidate = stage_initial(&mut runtime);
    assert_ne!(candidate.publication_token(), token);
    let installation = candidate.installation.clone();
    let mut foreign = Runtime::default();
    let existing = load_bytecode(&mut foreign, "initial", program()).unwrap();
    let existing_token = foreign.publication_token(&existing).unwrap();
    assert_eq!(
        foreign.publish_staged_load(candidate).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert!(runtime.modules.capture_current(&installation).is_err());
    assert_eq!(
        foreign.publication_token(&existing).unwrap(),
        existing_token
    );

    let candidate = stage_initial(&mut runtime);
    let installation = candidate.installation.clone();
    runtime.close().unwrap();
    assert!(runtime.publish_staged_load(candidate).is_err());
    assert!(runtime.modules.capture_current(&installation).is_err());
    let code = VerifiedProgram::new(program(), runtime.definition_context()).unwrap();
    assert!(runtime.stage_load_verified_program("closed", code).is_err());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForgottenScope {
    Execution,
    Scheduler,
    Publication,
}

#[rstest]
#[case::execution(ForgottenScope::Execution)]
#[case::scheduler(ForgottenScope::Scheduler)]
#[case::publication(ForgottenScope::Publication)]
fn staged_initial_load_admission_rejects_forgotten_scopes_and_publication_guards(
    #[case] mode: ForgottenScope,
) {
    let mut runtime = Runtime::default();
    let live = load_bytecode(&mut runtime, "existing", program()).unwrap();
    let candidate = stage_initial(&mut runtime);
    let installation = candidate.installation.clone();
    let code = VerifiedProgram::new(program(), runtime.definition_context()).unwrap();
    // Safe code cannot mutably borrow Runtime while a scope is live. Leaking
    // a scope ends that Rust borrow but must not bypass runtime admission.
    match mode {
        ForgottenScope::Execution => {
            let module = runtime.select_program(&live).unwrap();
            mem::forget(
                runtime
                    .begin_execution(&module, Default::default())
                    .unwrap(),
            );
        }
        ForgottenScope::Scheduler => {
            let cx = Context::from_waker(Waker::noop());
            mem::forget(runtime.begin_scheduler_turn(&cx, None).unwrap());
        }
        ForgottenScope::Publication => mem::forget(runtime.begin_publication(&live).unwrap()),
    }
    assert_eq!(
        runtime
            .stage_load_verified_program("reentrant", code)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert_eq!(
        runtime.publish_staged_load(candidate).unwrap_err().kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    assert!(runtime.modules.capture_current(&installation).is_err());
    drop(runtime);
}

#[test]
fn abandonment_invalidates_access_while_a_genuine_revision_root_retains_storage() {
    let mut runtime = Runtime::default();
    let baseline = load_bytecode(&mut runtime, "candidate", program()).unwrap();
    let candidate = stage(&mut runtime, &baseline);
    let module = candidate.module().clone();
    let retained = candidate.program.revision.clone();
    let view = runtime.modules.inner.borrow_mut();
    // Last-drop must not borrow a store that a caller is already inspecting.
    drop(candidate);
    assert_eq!(view.records[&module.key()].module.epoch, module.epoch);
    drop(view);
    assert!(runtime.validate_executable_module(&module).is_err());
    assert!(runtime.module_instance_snapshot(&module).is_none());
    assert!(runtime.modules.inner.borrow().resolve(&module).is_none());
    assert_eq!(
        runtime.modules.loaded_count(),
        runtime.select_program(&baseline).unwrap().members().count()
    );
    runtime.collect_garbage().unwrap();
    assert!(
        runtime
            .modules
            .inner
            .borrow()
            .records
            .contains_key(&module.key())
    );
    // Retaining physical storage does not undo admission invalidation. Once the
    // actual root is released, managed retirement also removes its metadata.
    thread::spawn(move || drop(retained)).join().unwrap();
    runtime.collect_garbage().unwrap();
    assert!(
        !runtime
            .modules
            .inner
            .borrow()
            .records
            .contains_key(&module.key())
    );
    assert_eq!(runtime.select_program(&baseline).unwrap().epoch.0, 1);
}

#[test]
fn publication_rejects_foreign_stores_and_forged_stage_leases() {
    let mut runtime = Runtime::default();
    let baseline = load_bytecode(&mut runtime, "candidate", program()).unwrap();
    let candidate = stage(&mut runtime, &baseline);
    let key = candidate.module().key();
    let forged = StagedProgram {
        revision: candidate.program.revision.clone(),
        installation: candidate.program.installation.clone(),
        expected_base: None,
        module: candidate.module().clone(),
        lease: Arc::new(CandidateLease::new(Arc::default(), false)),
    };
    assert_eq!(
        validation_kind(
            forged
                .publish(&runtime.modules, &mut runtime.memory, &runtime.resources)
                .unwrap_err()
        ),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(
        runtime
            .validate_executable_module(candidate.module())
            .is_ok()
    );
    let mut foreign = Runtime::default();
    let foreign_baseline = load_bytecode(&mut foreign, "candidate", program()).unwrap();
    let foreign_candidate = stage(&mut foreign, &foreign_baseline);
    assert_eq!(foreign_candidate.module().key(), key);
    assert_eq!(
        validation_kind(
            candidate
                .program
                .publish(&foreign.modules, &mut foreign.memory, &foreign.resources)
                .unwrap_err()
        ),
        RuntimeErrorKind::StaleHandle
    );
    assert!(runtime.modules.loaded(key).is_none());
    assert!(
        foreign
            .validate_executable_module(foreign_candidate.module())
            .is_ok()
    );
    let published_lease = foreign_candidate.program.lease.clone();
    foreign.publish_staged_reload(foreign_candidate).unwrap();
    thread::spawn(move || drop(published_lease)).join().unwrap();
    assert!(!foreign.modules.abandonment_pending());
}

#[test]
fn candidate_handles_do_not_keep_runtime_state_alive() {
    let mut runtime = Runtime::default();
    let baseline = load_bytecode(&mut runtime, "candidate", program()).unwrap();
    let candidate = stage(&mut runtime, &baseline);
    let resources = runtime.resources().lifetime_probe();
    drop(runtime);
    assert!(resources.upgrade().is_none());
    thread::spawn(move || drop(candidate)).join().unwrap();
}

#[test]
fn abandoned_programs_are_retired_at_safepoints_and_before_further_staging() {
    let mut runtime = Runtime::default();
    let baseline = load_bytecode(&mut runtime, "candidate", program()).unwrap();
    for _ in 0..32 {
        let candidate = stage(&mut runtime, &baseline);
        assert_eq!(runtime.modules.inner.borrow().records.len(), 2);
        assert!(!runtime.modules.abandonment_pending());
        let last = (
            candidate.program.lease.clone(),
            candidate.program.revision.clone(),
        );
        drop(candidate);
        assert!(!runtime.modules.abandonment_pending());
        thread::spawn(move || drop(last)).join().unwrap();
        assert!(runtime.modules.abandonment_pending());
        assert_eq!(runtime.modules.loaded_count(), 1);
    }
    runtime.gc_safepoint().unwrap();
    assert_eq!(runtime.modules.inner.borrow().records.len(), 1);
    assert_eq!(runtime.gc_stats().completed_cycles, 32);
    assert!(!runtime.modules.abandonment_pending());

    // Keep enough real source-owned allocations to observe a suspended sweep.
    // The candidate is marked while its staging root is still held; releasing it
    // now cannot reclaim it in that cycle and must request another fresh cycle.
    const COUNT: usize = 4096;
    let candidate = stage(&mut runtime, &baseline);
    let key = candidate.module().key();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                slots.grow(execution, ValueBank::Managed, COUNT)?;
                for slot in 0..COUNT {
                    let value = StringObject::alloc(slots.scope(), String::from("sweep witness"))?;
                    slots.set(
                        ValueLocation {
                            execution,
                            bank: ValueBank::Managed,
                            slot,
                        },
                        ScopedValue::managed(value),
                    )?;
                }
                Ok(())
            })
        })
        .unwrap();
    for _ in 0..128 {
        runtime.gc_safepoint().unwrap();
        if runtime.gc_stats().phase == Phase::Sweep {
            break;
        }
        assert_ne!(
            runtime.gc_stats().phase,
            Phase::Idle,
            "cycle must expose a pending sweep"
        );
    }
    assert_eq!(runtime.gc_stats().phase, Phase::Sweep);
    thread::spawn(move || drop(candidate)).join().unwrap();
    assert!(runtime.modules.abandonment_pending());
    for _ in 0..128 {
        runtime.gc_safepoint().unwrap();
        assert!(runtime.modules.abandonment_pending());
        if runtime.gc_stats().phase == Phase::Idle {
            break;
        }
    }
    assert_eq!(runtime.gc_stats().phase, Phase::Idle);
    assert!(runtime.modules.inner.borrow().records.contains_key(&key));
    runtime.gc_safepoint().unwrap();
    assert!(!runtime.modules.abandonment_pending());
    for _ in 0..128 {
        if runtime.gc_stats().phase == Phase::Idle {
            break;
        }
        runtime.gc_safepoint().unwrap();
    }
    assert_eq!(runtime.gc_stats().phase, Phase::Idle);
    assert!(!runtime.modules.inner.borrow().records.contains_key(&key));
    assert_eq!(runtime.modules.inner.borrow().records.len(), 1);
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
}

#[test]
fn disabled_automatic_gc_still_invalidates_candidates_until_explicit_collection() {
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = None;
    let mut runtime = Runtime::new(config);
    let baseline = load_bytecode(&mut runtime, "candidate", program()).unwrap();
    for _ in 0..4 {
        drop(stage(&mut runtime, &baseline));
    }
    runtime.gc_safepoint().unwrap();
    assert_eq!(runtime.modules.loaded_count(), 1);
    assert_eq!(runtime.gc_stats().completed_cycles, 0);
    assert_eq!(runtime.modules.inner.borrow().records.len(), 5);
    assert!(runtime.modules.abandonment_pending());
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.modules.inner.borrow().records.len(), 1);
    assert!(!runtime.modules.abandonment_pending());
}

#[test]
fn collection_rejects_borrowed_module_storage_before_detaching_candidates() {
    let mut runtime = Runtime::default();
    let baseline = load_bytecode(&mut runtime, "candidate", program()).unwrap();
    let candidate = stage(&mut runtime, &baseline);
    let selected = runtime.select_program(&baseline).unwrap();
    let view = runtime.modules.inner.borrow_mut();
    drop(candidate);
    assert_eq!(
        runtime
            .modules
            .check_collection_access()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(runtime.modules.abandonment_pending());
    assert_eq!(runtime.gc_stats().completed_cycles, 0);
    assert_eq!(view.records[&selected.key()].module.epoch, selected.epoch);
    assert!(!runtime.is_quarantined());
    drop(view);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.modules.inner.borrow().records.len(), 1);
    assert!(!runtime.modules.abandonment_pending());
}

#[test]
fn same_label_installations_have_independent_current_metadata_and_caches() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<InstalledProgram>();
    assert_send_sync::<Module>();

    let mut runtime = Runtime::default();
    let first = load_bytecode(&mut runtime, "same", program()).unwrap();
    let second = load_bytecode(&mut runtime, "same", program()).unwrap();
    let namespace = first.root();
    let old = runtime.select_program(&first).unwrap();
    let independent = runtime.select_program(&second).unwrap();
    assert_ne!(old.installation_id(), independent.installation_id());
    assert_ne!(old.id, independent.id);
    assert_eq!(old.epoch, independent.epoch);
    let first_cache = runtime
        .register_interpreter_cache(old.key(), None, old.verified_program().dependencies.clone())
        .unwrap();
    let second_cache = runtime
        .register_interpreter_cache(
            independent.key(),
            None,
            independent.verified_program().dependencies.clone(),
        )
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&first, Some("renamed".into()), program())
        .unwrap();
    let next = candidate.module().key();
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&first).unwrap();
    assert_eq!(current.key(), next);
    assert_eq!(current.id, old.id);
    assert_eq!(current.installation_id(), old.installation_id());
    assert_eq!(current.name, "renamed");
    assert_eq!(runtime.capture_module(&namespace).unwrap().root.key(), next);
    assert_eq!(
        runtime.select_program(&second).unwrap().key(),
        independent.key()
    );
    assert!(runtime.interpreter_cache(first_cache).is_none());
    assert!(runtime.interpreter_cache(second_cache).is_some());
    drop(first);
    drop(second);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules.loaded(old.key()).is_none());
    assert!(runtime.validate_executable_module(&old).is_err());
    assert_eq!(runtime.capture_module(&namespace).unwrap().root.key(), next);
}

#[test]
fn stale_candidates_cannot_overwrite_a_new_publication() {
    let mut runtime = Runtime::default();
    let target = load_bytecode(&mut runtime, "candidate", program()).unwrap();
    let original = runtime.select_program(&target).unwrap();
    let caller = load_bytecode(&mut runtime, "caller", program()).unwrap();
    let caller = runtime.select_program(&caller).unwrap();
    let roots_before = runtime.gc_stats().root_registrations;
    // A parked/admitted execution keeps actual state, rather than an old code
    // descriptor or a second Host root. Publication can replace current state.
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let body = {
        let Runtime {
            resources,
            memory,
            modules,
            ..
        } = &mut runtime;
        memory
            .scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let revision = RevisionSource::Current.resolve(modules, slots, &original)?;
                    slots.push_dependency(execution, revision)
                })
            })
            .unwrap()
    };
    assert_eq!(runtime.gc_stats().root_registrations, roots_before);
    let first = stage(&mut runtime, &target);
    let stale = stage(&mut runtime, &target);
    let stale_direct = stage(&mut runtime, &target);
    let next = first.module().key();
    runtime.publish_staged_reload(first).unwrap();
    let current = runtime.select_program(&target).unwrap();
    {
        // A different root calls an old admitted Native body. An ordinary inner
        // call selects the new publication; the top Native record cannot supply
        // its old revision merely because both versions share an installation.
        let mut session = runtime
            .begin_execution(&caller, Default::default())
            .unwrap();
        let runtime = session.runtime_mut();
        let active = runtime.resources.active_session().unwrap().id;
        let native = runtime
            .with_revision_origin(
                &original,
                RevisionSource::Execution(body),
                |slots, revision| {
                    let native = slots.enter_native(active)?;
                    let dependency = slots.push_dependency(active, revision)?;
                    slots.set_native_origin(&native, dependency)?;
                    Ok(native)
                },
            )
            .unwrap();
        let retained = RetentionOrigin::Admitted
            .resolve(runtime, &original)
            .unwrap();
        runtime
            .with_revision_origin(&original, retained, |slots, revision| {
                assert_eq!(
                    slots
                        .scope()
                        .read(revision, |revision| revision.code().key()),
                    original.program_root().key()
                );
                Ok(())
            })
            .unwrap();
        let mismatched = RetentionOrigin::Admitted
            .resolve(runtime, &current)
            .unwrap();
        assert_eq!(
            runtime
                .with_revision_origin(&current, mismatched, |_, _| Ok(()))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ModuleValidation
        );
        let fresh = runtime.selection_origin(&current).unwrap();
        runtime
            .with_revision_origin(&current, fresh, |slots, revision| {
                assert_eq!(
                    slots
                        .scope()
                        .read(revision, |revision| revision.code().key()),
                    current.program_root().key()
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots_before);
        assert!(!runtime.is_quarantined());
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| slots.retire_native(native))
            })
            .unwrap();
    }
    {
        let Runtime {
            resources,
            memory,
            modules,
            ..
        } = &mut runtime;
        memory
            .scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    assert!(matches!(
                        RevisionSource::Current.resolve(modules, slots, &original),
                        Err(GcError::Representation)
                    ));
                    let retained =
                        RevisionSource::Execution(body).resolve(modules, slots, &original)?;
                    assert_eq!(
                        slots
                            .scope()
                            .read(retained, |revision| revision.code().key()),
                        original.program_root().key()
                    );
                    let current = modules.current_for(&original).unwrap();
                    assert!(matches!(
                        RevisionSource::Execution(body).resolve(modules, slots, &current),
                        Err(GcError::Representation)
                    ));
                    let selected = RevisionSource::Current.resolve(modules, slots, &current)?;
                    assert!(!selected.ptr_eq(retained));
                    Ok(())
                })
            })
            .unwrap();
    }
    assert!(runtime.publish_staged_reload(stale).is_err());
    assert_eq!(
        validation_kind(
            stale_direct
                .program
                .publish(&runtime.modules, &mut runtime.memory, &runtime.resources)
                .unwrap_err()
        ),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(runtime.select_program(&target).unwrap().key(), next);
    let fresh = stage(&mut runtime, &target);
    assert!(fresh.module().epoch.0 > next.epoch.0 + 1);
    runtime.publish_staged_reload(fresh).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules.loaded(original.key()).is_some());
    assert_eq!(runtime.gc_stats().root_registrations, roots_before);
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules.loaded(original.key()).is_none());
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                assert!(matches!(slots.dependency(body), Err(GcError::Index)));
                Ok(())
            })
        })
        .unwrap();
}

#[test]
fn foreign_and_retired_handles_never_retarget_by_label() {
    let mut runtime = Runtime::default();
    let target = load_bytecode(&mut runtime, "same", program()).unwrap();
    let namespace = target.root();
    let mut foreign = Runtime::default();
    load_bytecode(&mut foreign, "same", program()).unwrap();
    assert_eq!(
        foreign.select_program(&target).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert!(
        foreign
            .stage_reload_program(&target, None, program())
            .is_err()
    );
    let candidate = stage(&mut runtime, &target);
    let code = Arc::downgrade(&candidate.baseline.program);
    // Retirement must not need an executable-storage borrow.
    let storage = runtime.modules.inner.borrow_mut();
    runtime
        .modules
        .revoke_publications(&mut runtime.memory, &runtime.resources)
        .unwrap();
    drop(storage);
    assert_eq!(
        runtime.capture_module(&namespace).err().unwrap().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(
        runtime.select_program(&target).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert!(runtime.publish_staged_reload(candidate).is_err());
    let root_after_retirement = target.root();
    assert_eq!(root_after_retirement.namespace, namespace.namespace);
    load_bytecode(&mut runtime, "same", program()).unwrap();
    assert_eq!(
        runtime.select_program(&target).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    drop(runtime);
    assert!(code.upgrade().is_none());
}

#[test]
fn rejected_close_preserves_liveness_but_drop_always_retires_installations() {
    let mut runtime = Runtime::default();
    let target = load_bytecode(&mut runtime, "candidate", program()).unwrap();
    let namespace = target.root();
    let selected = runtime.select_program(&target).unwrap();
    let code = Arc::downgrade(&selected.program);
    let key = selected.key();
    drop(selected);

    let context = Context::from_waker(Waker::noop());
    let turn = runtime.begin_scheduler_turn(&context, None).unwrap();
    mem::forget(turn);
    assert_eq!(
        runtime.close().unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.select_program(&target).unwrap().key(), key);
    assert_eq!(runtime.capture_module(&namespace).unwrap().root.key(), key);

    // A forgotten driver guard keeps close's precondition failing during Drop.
    // Destruction must still revoke the Runtime-owned current records.
    drop(runtime);
    assert!(code.upgrade().is_none());
}

#[test]
fn shutdown_fault_retires_handles_even_when_close_returns_an_error() {
    let mut runtime = Runtime::default();
    let target = load_bytecode(&mut runtime, "candidate", program()).unwrap();
    let namespace = target.root();
    let candidate = stage(&mut runtime, &target);
    runtime.quarantine_execution_invariant(EngineFault::ShutdownFaultTest);
    assert_eq!(
        runtime.close().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        runtime
            .modules
            .capture_current(&target.installation)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(
        runtime.capture_module(&namespace).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(
        runtime.select_program(&target).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert!(runtime.publish_staged_reload(candidate).is_err());
    assert_eq!(
        runtime.close().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(target.root().namespace, namespace.namespace);
}

#[test]
fn installation_and_member_identity_exhaustion_do_not_wrap() {
    let mut runtime = Runtime::default();
    runtime.modules.inner.get_mut().next_installation = u64::MAX;
    assert_eq!(
        load_bytecode(&mut runtime, "full", program())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
    assert!(runtime.modules.installations.is_empty());
    assert_eq!(runtime.modules.inner.get_mut().next_installation, u64::MAX);

    let mut runtime = Runtime::default();
    runtime.modules.inner.get_mut().next_id = u64::MAX;
    assert_eq!(
        load_bytecode(&mut runtime, "full", program())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
    assert_eq!(runtime.modules.inner.get_mut().next_id, u64::MAX);
    assert_eq!(runtime.modules.loaded_count(), 0);
}

/// Invalid stage authority must fail in metadata admission before GC root access.
fn validation_kind(error: ManagedProgramError) -> RuntimeErrorKind {
    match error {
        ManagedProgramError::Validation(error) => error.kind(),
        ManagedProgramError::Memory(error) => panic!("stage admission reached GC: {error}"),
    }
}
