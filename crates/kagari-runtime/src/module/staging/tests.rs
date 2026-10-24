use super::*;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime, RuntimeConfig, StagedLoad, StagedReload,
    error::RuntimeErrorKind,
    module::{Module, ModuleEpochRetention, Program, VerifiedProgram},
};
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use std::{
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
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

fn stage(runtime: &mut Runtime, baseline: &Program) -> StagedReload {
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
        installation.capture_current().unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(
        candidate.program.module().source_snapshot().publication(),
        token
    );
    assert!(runtime.modules.is_staged(candidate.program.module()));
    assert!(runtime.modules.current_roots().unwrap().is_empty());

    let live = runtime.publish_staged_load(candidate).unwrap();
    assert_eq!(runtime.publication_token(&live).unwrap(), token);
    assert_eq!(runtime.select_program(&live).unwrap().key(), key);
    assert_eq!(runtime.source_snapshot(&live).unwrap().publication(), token);
    assert_eq!(runtime.modules.current_roots().unwrap(), vec![key]);
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
    assert!(installation.capture_current().is_err());
    assert_eq!(
        runtime.collect_garbage().unwrap().reclaimed_modules,
        vec![module.key()]
    );

    let candidate = stage_initial(&mut runtime);
    assert_ne!(candidate.publication_token(), token);
    let installation = candidate.installation.clone();
    let mut foreign = Runtime::default();
    let existing = foreign.load_program("initial", program()).unwrap();
    let existing_token = foreign.publication_token(&existing).unwrap();
    assert_eq!(
        foreign.publish_staged_load(candidate).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert!(installation.capture_current().is_err());
    assert_eq!(
        foreign.publication_token(&existing).unwrap(),
        existing_token
    );

    let candidate = stage_initial(&mut runtime);
    let installation = candidate.installation.clone();
    runtime.close().unwrap();
    assert!(runtime.publish_staged_load(candidate).is_err());
    assert!(installation.capture_current().is_err());
    let code = VerifiedProgram::new(program(), runtime.definition_context()).unwrap();
    assert!(runtime.stage_load_verified_program("closed", code).is_err());
}

#[test]
fn staged_initial_load_admission_rejects_forgotten_scopes_and_publication_guards() {
    for mode in 0..3 {
        let mut runtime = Runtime::default();
        let live = runtime.load_program("existing", program()).unwrap();
        let candidate = stage_initial(&mut runtime);
        let installation = candidate.installation.clone();
        let code = VerifiedProgram::new(program(), runtime.definition_context()).unwrap();
        // Safe code cannot mutably borrow Runtime while a scope is live. Leaking
        // a scope ends that Rust borrow but must not bypass runtime admission.
        match mode {
            0 => {
                let module = runtime.select_program(&live).unwrap();
                mem::forget(
                    runtime
                        .begin_execution(&module, Default::default())
                        .unwrap(),
                );
            }
            1 => {
                let cx = Context::from_waker(Waker::noop());
                mem::forget(runtime.begin_scheduler_turn(&cx, None).unwrap());
            }
            _ => mem::forget(runtime.begin_publication(&live).unwrap()),
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
        assert!(installation.capture_current().is_err());
        drop(runtime);
        assert!(live.capture_current().is_err());
    }
}

#[test]
fn abandonment_invalidates_access_even_with_a_retention_lease() {
    let mut runtime = Runtime::default();
    let baseline = runtime.load_program("candidate", program()).unwrap();
    let candidate = stage(&mut runtime, &baseline);
    let module = candidate.module().clone();
    let lease = runtime
        .retain_module(&module, ModuleEpochRetention::RuntimeValue)
        .unwrap();
    let view = runtime.modules.inner.borrow_mut();
    // Last-drop must not borrow a store that a caller is already inspecting.
    drop(candidate);
    assert_eq!(view.records[&module.key()].module.epoch, module.epoch);
    drop(view);
    assert!(runtime.validate_executable_module(&module).is_err());
    assert!(runtime.modules.instance_snapshot(module.key()).is_none());
    assert!(runtime.modules.inner.borrow().resolve(&module).is_none());
    assert!(
        runtime
            .retain_module(&module, ModuleEpochRetention::ActiveCall)
            .is_none()
    );
    assert_eq!(
        runtime.modules.loaded_count(),
        runtime.select_program(&baseline).unwrap().members().count()
    );
    assert_eq!(runtime.modules.retention_counts(module.key()).total(), 0);
    assert_eq!(
        runtime.collect_garbage().unwrap().reclaimed_modules,
        vec![module.key()]
    );
    assert!(
        !runtime
            .modules
            .inner
            .borrow()
            .records
            .contains_key(&module.key())
    );
    assert_eq!(runtime.select_program(&baseline).unwrap().epoch.0, 1);
    drop(lease);
}

#[test]
fn publication_rejects_foreign_stores_and_forged_stage_leases() {
    let mut runtime = Runtime::default();
    let baseline = runtime.load_program("candidate", program()).unwrap();
    let candidate = stage(&mut runtime, &baseline);
    let key = candidate.module().key();
    let forged = StagedProgram {
        installation: candidate.program.installation.clone(),
        expected_base: None,
        module: candidate.module().clone(),
        lease: Arc::new(CandidateLease::new(Arc::default(), false)),
    };
    assert_eq!(
        forged.publish(&runtime.modules).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(
        runtime
            .validate_executable_module(candidate.module())
            .is_ok()
    );
    let mut foreign = Runtime::default();
    let foreign_baseline = foreign.load_program("candidate", program()).unwrap();
    let foreign_candidate = stage(&mut foreign, &foreign_baseline);
    assert_eq!(foreign_candidate.module().key(), key);
    assert_eq!(
        candidate
            .program
            .publish(&foreign.modules)
            .unwrap_err()
            .kind(),
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
    let baseline = runtime.load_program("candidate", program()).unwrap();
    let candidate = stage(&mut runtime, &baseline);
    let resources = runtime.resources().lifetime_probe();
    drop(runtime);
    assert!(resources.upgrade().is_none());
    thread::spawn(move || drop(candidate)).join().unwrap();
}

#[test]
fn abandoned_programs_are_retired_at_safepoints_and_before_further_staging() {
    let mut runtime = Runtime::default();
    let baseline = runtime.load_program("candidate", program()).unwrap();
    for _ in 0..32 {
        let candidate = stage(&mut runtime, &baseline);
        assert_eq!(runtime.modules.inner.borrow().records.len(), 2);
        assert!(!runtime.modules.abandonment_pending());
        let last = candidate.program.lease.clone();
        drop(candidate);
        assert!(!runtime.modules.abandonment_pending());
        thread::spawn(move || drop(last)).join().unwrap();
        assert!(runtime.modules.abandonment_pending());
        assert_eq!(runtime.modules.loaded_count(), 1);
    }
    runtime.gc_safepoint().unwrap();
    assert_eq!(runtime.modules.inner.borrow().records.len(), 1);
    assert_eq!(runtime.gc.stats().collections, 32);
    assert!(!runtime.modules.abandonment_pending());

    // A failed/incomplete graph walk must not consume a pending request.
    let candidate = stage(&mut runtime, &baseline);
    drop(candidate);
    let graph = runtime.modules.collection_graph().unwrap();
    assert!(!runtime.modules.abandonment_pending());
    drop(graph);
    assert!(runtime.modules.abandonment_pending());
    runtime.gc_safepoint().unwrap();
    assert!(!runtime.modules.abandonment_pending());

    // Last release after root discovery needs another collection even when
    // that program was already retained by the in-progress mark.
    let candidate = stage(&mut runtime, &baseline);
    let mut graph = runtime.modules.collection_graph().unwrap();
    let roots = graph.roots();
    thread::spawn(move || drop(candidate)).join().unwrap();
    let dead = graph.prepare_sweep(&roots);
    assert!(graph.detach(dead).is_empty());
    drop(graph);
    assert!(runtime.modules.abandonment_pending());
    runtime.gc_safepoint().unwrap();
    assert!(!runtime.modules.abandonment_pending());
    assert_eq!(runtime.modules.inner.borrow().records.len(), 1);
}

#[test]
fn disabled_automatic_gc_still_invalidates_candidates_until_explicit_collection() {
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = None;
    let mut runtime = Runtime::new(config);
    let baseline = runtime.load_program("candidate", program()).unwrap();
    for _ in 0..4 {
        drop(stage(&mut runtime, &baseline));
    }
    runtime.gc_safepoint().unwrap();
    assert_eq!(runtime.modules.loaded_count(), 1);
    assert_eq!(runtime.gc.stats().collections, 0);
    assert_eq!(runtime.modules.inner.borrow().records.len(), 5);
    assert!(runtime.modules.abandonment_pending());
    assert_eq!(
        runtime.collect_garbage().unwrap().reclaimed_modules.len(),
        4
    );
    assert_eq!(runtime.modules.inner.borrow().records.len(), 1);
    assert!(!runtime.modules.abandonment_pending());
}

#[test]
fn safepoints_reject_borrowed_module_storage_without_panicking() {
    for automatic in [true, false] {
        let mut config = RuntimeConfig::default();
        config.gc.collection_threshold = automatic.then_some(1024);
        let mut runtime = Runtime::new(config);
        let baseline = runtime.load_program("candidate", program()).unwrap();
        let view = runtime.modules.inner.borrow_mut();
        // Public safepoints retain borrow validation without pending work too.
        assert_eq!(runtime.gc_safepoint().is_err(), automatic);
        drop(view);
        let candidate = stage(&mut runtime, &baseline);
        let view = runtime.modules.inner.borrow_mut();
        drop(candidate);
        let result = runtime.gc_safepoint();
        if automatic {
            assert_eq!(
                result.unwrap_err().kind(),
                RuntimeErrorKind::ModuleValidation
            );
        } else {
            result.unwrap();
        }
        let executable = baseline.capture_current().unwrap();
        assert_eq!(
            view.records[&executable.key()].module.epoch,
            executable.epoch
        );
        assert!(!runtime.is_quarantined());
        drop(view);
        assert_eq!(
            runtime.collect_garbage().unwrap().reclaimed_modules.len(),
            1
        );
    }
}

#[test]
fn same_label_installations_have_independent_current_metadata_and_caches() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Program>();
    assert_send_sync::<Module>();

    let mut runtime = Runtime::default();
    let first = runtime.load_program("same", program()).unwrap();
    let second = runtime.load_program("same", program()).unwrap();
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
    assert_eq!(namespace.capture_current().unwrap().root.key(), next);
    assert_eq!(
        runtime.select_program(&second).unwrap().key(),
        independent.key()
    );
    assert!(runtime.interpreter_cache(first_cache).is_none());
    assert!(runtime.interpreter_cache(second_cache).is_some());
    drop(first);
    drop(second);
    assert_eq!(
        runtime.collect_garbage().unwrap().reclaimed_modules,
        vec![old.key()]
    );
    assert!(runtime.validate_executable_module(&old).is_err());
    assert_eq!(namespace.capture_current().unwrap().root.key(), next);
}

#[test]
fn stale_candidates_cannot_overwrite_a_new_publication() {
    let mut runtime = Runtime::default();
    let target = runtime.load_program("candidate", program()).unwrap();
    let first = stage(&mut runtime, &target);
    let stale = stage(&mut runtime, &target);
    let stale_direct = stage(&mut runtime, &target);
    let next = first.module().key();
    runtime.publish_staged_reload(first).unwrap();
    assert!(runtime.publish_staged_reload(stale).is_err());
    assert_eq!(
        stale_direct
            .program
            .publish(&runtime.modules)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(runtime.select_program(&target).unwrap().key(), next);
    let fresh = stage(&mut runtime, &target);
    assert!(fresh.module().epoch.0 > next.epoch.0 + 1);
    runtime.publish_staged_reload(fresh).unwrap();
}

#[test]
fn foreign_and_retired_handles_never_retarget_by_label() {
    let mut runtime = Runtime::default();
    let target = runtime.load_program("same", program()).unwrap();
    let namespace = target.root();
    let mut foreign = Runtime::default();
    foreign.load_program("same", program()).unwrap();
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
    runtime.modules.retire_installations();
    drop(storage);
    assert_eq!(
        namespace.capture_current().err().unwrap().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(
        runtime.select_program(&target).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert!(runtime.publish_staged_reload(candidate).is_err());
    let root_after_retirement = target.root();
    assert_eq!(root_after_retirement.namespace, namespace.namespace);
    runtime.load_program("same", program()).unwrap();
    assert_eq!(
        runtime.select_program(&target).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    drop(runtime);
    assert!(code.upgrade().is_none());
}

#[test]
fn poisoned_cells_fail_admission_and_retirement_only_clears_authority() {
    let mut runtime = Runtime::default();
    let target = runtime.load_program("candidate", program()).unwrap();
    let candidate = stage(&mut runtime, &target);
    let before = Arc::downgrade(&candidate.baseline.program);
    let installation = target.installation.clone();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _guard = installation.current.lock().unwrap();
            panic!("poison metadata for admission test");
        }))
        .is_err()
    );
    assert_eq!(
        target.capture_current().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        runtime.select_program(&target).unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    let error = candidate.program.publish(&runtime.modules).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    runtime.modules.retire_installations();
    assert!(
        installation
            .current
            .lock()
            .unwrap_err()
            .into_inner()
            .is_none()
    );
    // Recovery never clears poison or re-admits the former publication.
    assert_eq!(
        target.capture_current().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    drop(candidate.baseline);
    drop(runtime);
    assert!(before.upgrade().is_none());
}

#[test]
fn rejected_close_preserves_liveness_but_drop_always_retires_installations() {
    let mut runtime = Runtime::default();
    let target = runtime.load_program("candidate", program()).unwrap();
    let namespace = target.root();
    let selected = runtime.select_program(&target).unwrap();
    let code = Arc::downgrade(&selected.program);
    let key = selected.key();
    drop(selected);

    let context = Context::from_waker(Waker::noop());
    let turn = runtime.begin_scheduler_turn(&context, None).unwrap();
    assert_eq!(
        runtime.close().unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.select_program(&target).unwrap().key(), key);
    assert_eq!(namespace.capture_current().unwrap().root.key(), key);

    // A forgotten driver guard keeps close's precondition failing during Drop.
    // Destruction must still revoke the independent installation cells.
    mem::forget(turn);
    drop(runtime);
    assert_eq!(
        target.capture_current().unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(
        namespace.capture_current().unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert!(code.upgrade().is_none());
}

#[test]
fn shutdown_fault_retires_handles_even_when_close_returns_an_error() {
    let mut runtime = Runtime::default();
    let target = runtime.load_program("candidate", program()).unwrap();
    let namespace = target.root();
    let candidate = stage(&mut runtime, &target);
    runtime.quarantine_execution_invariant("shutdown fault test");
    assert_eq!(
        runtime.close().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        target.capture_current().unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(
        namespace.capture_current().unwrap_err().kind(),
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
        runtime.load_program("full", program()).unwrap_err().kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
    assert!(runtime.modules.installations.is_empty());
    assert_eq!(runtime.modules.inner.get_mut().next_installation, u64::MAX);

    let mut runtime = Runtime::default();
    runtime.modules.inner.get_mut().next_id = u64::MAX;
    assert_eq!(
        runtime.load_program("full", program()).unwrap_err().kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
    assert_eq!(runtime.modules.inner.get_mut().next_id, u64::MAX);
    assert_eq!(runtime.modules.loaded_count(), 0);
}
