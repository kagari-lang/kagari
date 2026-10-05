//! Version retention is registered centrally; exported leases never own the store.
use crate::module::{ModuleEpochRetention, ModuleEpochRetentionCounts};
use std::{
    collections::TryReserveError,
    sync::{Arc, Weak},
};

#[derive(Debug)]
struct Retention {
    kind: ModuleEpochRetention,
}

/// Keeps registered module versions reachable until the last clone is dropped.
/// A lease has no access to module storage and may outlive its runtime.
#[must_use = "retain the lease while the selected versions are needed"]
#[derive(Debug, Clone)]
pub struct ProgramLease {
    retention: Arc<Retention>,
}

impl ProgramLease {
    pub(super) fn new(kind: ModuleEpochRetention) -> Self {
        Self {
            retention: Arc::new(Retention { kind }),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Retentions(Vec<Weak<Retention>>);

impl Retentions {
    pub(super) fn register(&mut self, lease: &ProgramLease) -> Result<(), TryReserveError> {
        self.prune();
        self.0.try_reserve(1)?;
        self.0.push(Arc::downgrade(&lease.retention));
        Ok(())
    }

    pub(super) fn prune(&mut self) {
        self.0.retain(|lease| lease.strong_count() != 0);
    }

    pub(super) fn is_retained(&self) -> bool {
        self.0.iter().any(|lease| lease.strong_count() != 0)
    }

    pub(super) fn counts(&self) -> ModuleEpochRetentionCounts {
        let mut counts = ModuleEpochRetentionCounts::default();
        for lease in self.0.iter().filter_map(Weak::upgrade) {
            match lease.kind {
                ModuleEpochRetention::ActiveCall => counts.active_calls += 1,
                ModuleEpochRetention::RuntimeValue => counts.runtime_values += 1,
                ModuleEpochRetention::CompiledArtifact => counts.compiled_artifacts += 1,
            }
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Runtime, error::RuntimeErrorKind, module::LoadedModule, value::Value};
    use kagari_bytecode::{
        instruction::ModuleSlot,
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

    fn load(runtime: &mut Runtime) -> LoadedModule {
        runtime.load_program("retention", program()).unwrap()
    }

    #[test]
    fn foreign_and_released_modules_cannot_register_retention() {
        let mut runtime = Runtime::default();
        let own = load(&mut runtime);
        let mut foreign_runtime = Runtime::default();
        let foreign = load(&mut foreign_runtime);
        assert_eq!(own.key(), foreign.key());
        assert!(
            runtime
                .retain_module(&foreign, ModuleEpochRetention::ActiveCall)
                .is_none()
        );
        assert!(
            runtime
                .retain_program(&foreign, ModuleEpochRetention::RuntimeValue)
                .is_none()
        );
        assert_eq!(runtime.modules().retention_counts(own.key()).total(), 0);
        let candidate = runtime
            .stage_reload_program(&own, "retention", program())
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
        assert_eq!(
            runtime.collect_garbage().unwrap().reclaimed_modules,
            vec![own.key()]
        );
        assert!(
            runtime
                .retain_module(&own, ModuleEpochRetention::ActiveCall)
                .is_none()
        );
        assert!(
            runtime
                .retain_program(&own, ModuleEpochRetention::CompiledArtifact)
                .is_none()
        );
    }

    #[test]
    fn cloned_leases_pin_one_registration_until_the_last_cross_thread_drop() {
        let mut runtime = Runtime::default();
        let first = load(&mut runtime);
        let lease = runtime
            .retain_program(&first, ModuleEpochRetention::CompiledArtifact)
            .unwrap();
        let clone = lease.clone();
        let clone = thread::spawn(move || clone).join().unwrap();
        assert_eq!(
            runtime
                .modules()
                .retention_counts(first.key())
                .compiled_artifacts,
            1
        );
        let candidate = runtime
            .stage_reload_program(&first, "retention", program())
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
        drop(lease);
        assert!(
            runtime
                .collect_garbage()
                .unwrap()
                .reclaimed_modules
                .is_empty()
        );
        assert!(runtime.modules().is_program_root(first.key()));
        thread::spawn(move || drop(clone)).join().unwrap();
        assert_eq!(runtime.modules().retention_counts(first.key()).total(), 0);
        assert_eq!(
            runtime.collect_garbage().unwrap().reclaimed_modules,
            vec![first.key()]
        );
    }

    #[test]
    fn lease_release_needs_no_store_borrow_and_leases_do_not_own_storage() {
        let mut runtime = Runtime::default();
        let module = load(&mut runtime);
        let lease = runtime
            .retain_module(&module, ModuleEpochRetention::ActiveCall)
            .unwrap();
        let instance = runtime.modules.instance_mut(module.key()).unwrap();
        drop(lease);
        drop(instance);
        assert_eq!(
            runtime
                .modules()
                .retention_counts(module.key())
                .active_calls,
            0
        );
        let lease = runtime
            .retain_module(&module, ModuleEpochRetention::RuntimeValue)
            .unwrap();
        let code = Arc::downgrade(&module.program);
        drop(module);
        drop(runtime);
        assert!(code.upgrade().is_none());
        thread::spawn(move || drop(lease)).join().unwrap();
    }

    #[test]
    fn retention_during_collection_cannot_resurrect_programs() {
        for whole_program in [false, true] {
            let mut runtime = Runtime::default();
            let module = load(&mut runtime);
            let error = runtime
                .resources()
                .collect_operation(|| {
                    let lease = if whole_program {
                        runtime.retain_program(&module, ModuleEpochRetention::CompiledArtifact)
                    } else {
                        runtime.retain_module(&module, ModuleEpochRetention::RuntimeValue)
                    };
                    assert!(lease.is_none());
                })
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
            assert!(runtime.is_quarantined());
            assert_eq!(runtime.modules().retention_counts(module.key()).total(), 0);
        }
    }

    #[test]
    fn quarantine_blocks_state_access_retention_and_publication() {
        let mut runtime = Runtime::default();
        let baseline = load(&mut runtime);
        let candidate = runtime
            .stage_reload_program(&baseline, "retention", program())
            .unwrap();
        runtime.resources().quarantine("test execution fault");
        assert!(runtime.module_instance_snapshot(&baseline).is_none());
        assert_eq!(
            runtime
                .write_module_slot(&baseline, ModuleSlot::new(0), Value::Unit)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        assert!(
            runtime
                .retain_module(&baseline, ModuleEpochRetention::ActiveCall)
                .is_none()
        );
        assert!(runtime.publish_staged_reload(candidate).is_err());
        assert_eq!(
            runtime.load_program("new", program()).unwrap_err().kind(),
            RuntimeErrorKind::EngineFault
        );
        assert!(runtime.modules().latest("new").is_none());
        assert_eq!(
            runtime.modules().latest("retention").unwrap().key(),
            baseline.key()
        );
        assert_eq!(runtime.modules().loaded_count(), 1);
    }
}
