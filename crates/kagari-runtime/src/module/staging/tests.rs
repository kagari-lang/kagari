use super::*;
use crate::{
    Runtime, RuntimeConfig, StagedReload, error::RuntimeErrorKind, module::ModuleEpochRetention,
};
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use std::thread;

fn program() -> BytecodeProgram {
    BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    }
}

fn stage(runtime: &mut Runtime, baseline: &LoadedModule) -> StagedReload {
    runtime
        .stage_reload_program(baseline, "candidate", program())
        .unwrap()
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
    let view = runtime.modules.instance_mut(module.key()).unwrap();
    // Last-drop must not borrow a store that a caller is already inspecting.
    drop(candidate);
    assert_eq!(view.epoch, module.epoch);
    drop(view);
    assert!(runtime.validate_loaded_module(&module).is_err());
    assert!(runtime.modules.instance_snapshot(module.key()).is_none());
    assert!(runtime.modules.instance_mut(module.key()).is_none());
    assert!(
        runtime
            .retain_module(&module, ModuleEpochRetention::ActiveCall)
            .is_none()
    );
    assert_eq!(runtime.modules.loaded_count(), baseline.members().count());
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
    assert_eq!(
        runtime.modules.latest("candidate").unwrap().key(),
        baseline.key()
    );
    drop(lease);
}

#[test]
fn publication_rejects_foreign_stores_and_forged_stage_leases() {
    let mut runtime = Runtime::default();
    let baseline = runtime.load_program("candidate", program()).unwrap();
    let candidate = stage(&mut runtime, &baseline);
    let key = candidate.module().key();
    let forged = StagedProgram {
        module: candidate.module().clone(),
        lease: Arc::new(CandidateLease::new(Arc::default())),
    };
    assert_eq!(
        forged.publish(&runtime.modules).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(runtime.validate_loaded_module(candidate.module()).is_ok());
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
        RuntimeErrorKind::ModuleValidation
    );
    assert!(runtime.modules.loaded(key).is_none());
    assert!(
        foreign
            .validate_loaded_module(foreign_candidate.module())
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
        let view = runtime.modules.instance_mut(baseline.key()).unwrap();
        // Public safepoints retain borrow validation without pending work too.
        assert_eq!(runtime.gc_safepoint().is_err(), automatic);
        drop(view);
        let candidate = stage(&mut runtime, &baseline);
        let view = runtime.modules.instance_mut(baseline.key()).unwrap();
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
        assert_eq!(view.epoch, baseline.epoch);
        assert!(!runtime.is_quarantined());
        drop(view);
        assert_eq!(
            runtime.collect_garbage().unwrap().reclaimed_modules.len(),
            1
        );
    }
}
