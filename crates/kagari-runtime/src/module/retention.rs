//! Version retention is registered centrally; exported leases never own the store.
use crate::module::{ModuleEpochRetention, ModuleEpochRetentionCounts};
use std::sync::Arc;

/// Keeps registered module versions reachable until the last clone is dropped.
/// A lease has no access to module storage and may outlive its runtime.
#[must_use = "retain the lease while the selected versions are needed"]
#[derive(Debug, Clone)]
pub struct ProgramLease {
    _registration: Arc<Registration>,
}

/// Lease clones share this registration. Only distinct registrations hold a
/// token reference, so clones do not increase the store's registration count.
#[derive(Debug)]
struct Registration {
    _token: Arc<()>,
}

/// Tokens own no program or runtime state. The store holds one reference; each
/// distinct registration holds one more until its last lease clone is dropped.
#[derive(Debug, Default)]
struct Registrations {
    active_calls: Option<Arc<()>>,
    runtime_values: Option<Arc<()>>,
    compiled_artifacts: Option<Arc<()>>,
}

#[derive(Debug, Default)]
pub(super) struct Retentions {
    member: Registrations,
    // Only a program root needs whole-program registrations.
    program: Option<Box<Registrations>>,
}

impl Retentions {
    pub(super) fn retain_member(&mut self, kind: ModuleEpochRetention) -> ProgramLease {
        self.member.retain(kind)
    }

    pub(super) fn retain_program(&mut self, kind: ModuleEpochRetention) -> ProgramLease {
        self.program.get_or_insert_default().retain(kind)
    }

    pub(super) fn is_retained(&self) -> bool {
        self.member.counts().is_retained() || self.program_counts().is_retained()
    }

    pub(super) fn member_counts(&self) -> ModuleEpochRetentionCounts {
        self.member.counts()
    }

    pub(super) fn program_counts(&self) -> ModuleEpochRetentionCounts {
        self.program
            .as_deref()
            .map(Registrations::counts)
            .unwrap_or_default()
    }
}

impl Registrations {
    fn retain(&mut self, kind: ModuleEpochRetention) -> ProgramLease {
        let token = match kind {
            ModuleEpochRetention::ActiveCall => &mut self.active_calls,
            ModuleEpochRetention::RuntimeValue => &mut self.runtime_values,
            ModuleEpochRetention::CompiledArtifact => &mut self.compiled_artifacts,
        }
        .get_or_insert_with(|| Arc::new(()));
        ProgramLease {
            _registration: Arc::new(Registration {
                _token: token.clone(),
            }),
        }
    }

    fn counts(&self) -> ModuleEpochRetentionCounts {
        // New registrations require the store's exclusive borrow. Other threads
        // can only release token references, so a snapshot may retain too much,
        // never omit a registration created concurrently with collection.
        let count = |token: &Option<Arc<()>>| {
            token
                .as_ref()
                .map_or(0, |token| Arc::strong_count(token) - 1)
        };
        ModuleEpochRetentionCounts {
            active_calls: count(&self.active_calls),
            runtime_values: count(&self.runtime_values),
            compiled_artifacts: count(&self.compiled_artifacts),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::native::program::ProgramFixture;
    use crate::{
        Runtime,
        error::RuntimeErrorKind,
        module::{ExecutableModule, Program},
        value::Value,
    };
    use kagari_bytecode::{
        instruction::ModuleSlot,
        module::BytecodeModule,
        program::{BytecodeProgram, ModuleRef},
    };
    use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
    use std::thread;

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

    fn load(runtime: &mut Runtime) -> (Program, ExecutableModule) {
        let program = runtime.load_program("retention", program()).unwrap();
        let executable = runtime.select_program(&program).unwrap();
        (program, executable)
    }

    #[test]
    fn foreign_and_released_modules_cannot_register_retention() {
        let mut runtime = Runtime::default();
        let (target, own) = load(&mut runtime);
        let mut foreign_runtime = Runtime::default();
        let (_foreign_program, foreign) = load(&mut foreign_runtime);
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
            .stage_reload_program(&target, None, program())
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
        let (target, first) = load(&mut runtime);
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
            .stage_reload_program(&target, None, program())
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
        let (_program, module) = load(&mut runtime);
        let lease = runtime
            .retain_module(&module, ModuleEpochRetention::ActiveCall)
            .unwrap();
        let instance = runtime.modules.inner.borrow_mut();
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
            let (_program, module) = load(&mut runtime);
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
        let (target, baseline) = load(&mut runtime);
        let candidate = runtime
            .stage_reload_program(&target, None, program())
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
        assert_eq!(target.capture_current().unwrap().key(), baseline.key());
        assert_eq!(runtime.modules().loaded_count(), 1);
    }
}
