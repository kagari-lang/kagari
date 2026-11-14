//! Host leases retain actual revision roots and share diagnostic registrations.
#[cfg(test)]
use crate::error::fault::EngineFault;
use crate::module::{
    ExecutableModule, ModuleEpochRetention, ModuleEpochRetentionCounts,
    managed::{revision::RevisionState, source::RevisionSource},
};
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{Runtime, error::RuntimeError};
use kagari_gc::root::Root;
use std::sync::Arc;

/// Select current publication, borrow an admitted body, or reopen an exact
/// retained revision. A descriptor alone grants no access to retired state.
#[derive(Debug, Clone, Copy)]
pub enum RetentionOrigin<'lease> {
    Current,
    /// Borrow the exact body already admitted in this Runtime's active execution.
    /// Missing or mismatched active bodies fail; this never selects publication.
    Admitted,
    Retained(&'lease ProgramLease),
}

impl<'lease> RetentionOrigin<'lease> {
    pub(crate) fn resolve(
        self,
        runtime: &Runtime,
        owner: &ExecutableModule,
    ) -> Result<RevisionSource<'lease>, RuntimeError> {
        Ok(match self {
            Self::Current => RevisionSource::Current,
            Self::Retained(lease) => RevisionSource::Host(lease.revision()),
            Self::Admitted => {
                RevisionSource::Execution(runtime.admitted_origin(owner)?.ok_or_else(|| {
                    RuntimeError::module_validation("no matching admitted execution body")
                })?)
            }
        })
    }
}

/// Keeps registered module versions reachable until the last clone is dropped.
/// A lease has no access to module storage and may outlive its runtime.
#[must_use = "retain the lease while the selected versions are needed"]
#[derive(Debug, Clone)]
pub struct ProgramLease {
    _registration: Arc<RetainedProgram>,
}

/// Lease clones share this registration. Only distinct registrations hold a
/// token reference, so clones do not increase the store's registration count.
#[derive(Debug)]
struct RetainedProgram {
    _registration: RetentionRegistration,
    revision: Root<RevisionState>,
}

/// Diagnostic lifetime only. This token grants no GC access or retention; the
/// execution source independently owns the admitted body's actual revision edge.
#[derive(Debug)]
pub(crate) struct RetentionRegistration {
    _token: Arc<()>,
}

impl ProgramLease {
    fn new(registration: RetentionRegistration, revision: Root<RevisionState>) -> Self {
        Self {
            _registration: Arc::new(RetainedProgram {
                _registration: registration,
                revision,
            }),
        }
    }

    /// Borrow the genuine retained origin without creating another root lease.
    pub(crate) fn revision(&self) -> &Root<RevisionState> {
        &self._registration.revision
    }
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
    pub(super) fn retain_member(
        &mut self,
        kind: ModuleEpochRetention,
        revision: Root<RevisionState>,
    ) -> ProgramLease {
        ProgramLease::new(self.member.register(kind), revision)
    }

    pub(super) fn retain_program(
        &mut self,
        kind: ModuleEpochRetention,
        revision: Root<RevisionState>,
    ) -> ProgramLease {
        ProgramLease::new(
            self.program.get_or_insert_default().register(kind),
            revision,
        )
    }

    pub(super) fn register_execution(&mut self) -> RetentionRegistration {
        self.member.register(ModuleEpochRetention::ActiveCall)
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
    fn register(&mut self, kind: ModuleEpochRetention) -> RetentionRegistration {
        let token = match kind {
            ModuleEpochRetention::ActiveCall => &mut self.active_calls,
            ModuleEpochRetention::RuntimeValue => &mut self.runtime_values,
            ModuleEpochRetention::CompiledArtifact => &mut self.compiled_artifacts,
        }
        .get_or_insert_with(|| Arc::new(()));
        RetentionRegistration {
            _token: token.clone(),
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
        module::{ExecutableModule, InstalledProgram},
        value::carriers::RootedValue,
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

    fn load(runtime: &mut Runtime) -> (InstalledProgram, ExecutableModule) {
        let program = load_bytecode(runtime, "retention", program()).unwrap();
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
                .retain_module(
                    &foreign,
                    ModuleEpochRetention::ActiveCall,
                    RetentionOrigin::Current
                )
                .is_none()
        );
        assert!(
            runtime
                .retain_program(
                    &foreign,
                    ModuleEpochRetention::RuntimeValue,
                    RevisionSource::Current
                )
                .is_none()
        );
        let foreign_lease = foreign_runtime
            .retain_module(
                &foreign,
                ModuleEpochRetention::RuntimeValue,
                RetentionOrigin::Current,
            )
            .unwrap();
        assert!(
            runtime
                .retain_module(
                    &own,
                    ModuleEpochRetention::RuntimeValue,
                    RetentionOrigin::Retained(&foreign_lease),
                )
                .is_none()
        );
        assert!(!runtime.is_quarantined());
        assert_eq!(runtime.modules().retention_counts(own.key()).total(), 0);
        let candidate = runtime
            .stage_reload_program(&target, None, program())
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
        runtime.collect_garbage().unwrap();
        assert!(runtime.modules().loaded(own.key()).is_none());
        assert!(
            runtime
                .retain_module(
                    &own,
                    ModuleEpochRetention::ActiveCall,
                    RetentionOrigin::Current
                )
                .is_none()
        );
        assert!(
            runtime
                .retain_program(
                    &own,
                    ModuleEpochRetention::CompiledArtifact,
                    RevisionSource::Current
                )
                .is_none()
        );
    }

    #[test]
    fn cloned_leases_pin_one_registration_until_the_last_cross_thread_drop() {
        let mut runtime = Runtime::default();
        let (target, first) = load(&mut runtime);
        let lease = runtime
            .retain_program(
                &first,
                ModuleEpochRetention::CompiledArtifact,
                RevisionSource::Current,
            )
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
        runtime.collect_garbage().unwrap();
        assert!(runtime.modules().loaded(first.key()).is_some());
        let roots = runtime.memory.stats().root_registrations;
        assert!(
            runtime
                .retain_module(
                    &first,
                    ModuleEpochRetention::RuntimeValue,
                    RetentionOrigin::Current,
                )
                .is_none(),
            "current publication cannot authenticate an old body"
        );
        let retained = runtime
            .retain_module(
                &first,
                ModuleEpochRetention::RuntimeValue,
                RetentionOrigin::Retained(&clone),
            )
            .unwrap();
        assert_eq!(runtime.memory.stats().root_registrations, roots);
        assert_eq!(
            runtime
                .modules()
                .retention_counts(first.key())
                .runtime_values,
            1
        );
        runtime
            .with_revision_origin(
                &first,
                RevisionSource::Host(retained.revision()),
                |slots, revision| {
                    assert_eq!(
                        slots
                            .scope()
                            .read(revision, |revision| revision.code().key()),
                        first.key()
                    );
                    Ok(())
                },
            )
            .unwrap();
        thread::spawn(move || drop(clone)).join().unwrap();
        runtime.collect_garbage().unwrap();
        assert_eq!(
            runtime
                .modules()
                .retention_counts(first.key())
                .compiled_artifacts,
            0
        );
        assert!(runtime.modules().loaded(first.key()).is_some());
        drop(retained);
        assert_eq!(runtime.modules().retention_counts(first.key()).total(), 0);
        runtime.collect_garbage().unwrap();
        assert!(runtime.modules().loaded(first.key()).is_none());
    }

    #[test]
    fn lease_release_needs_no_store_borrow_and_leases_do_not_own_storage() {
        let mut runtime = Runtime::default();
        let (_program, module) = load(&mut runtime);
        let lease = runtime
            .retain_module(
                &module,
                ModuleEpochRetention::ActiveCall,
                RetentionOrigin::Current,
            )
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
            .retain_module(
                &module,
                ModuleEpochRetention::RuntimeValue,
                RetentionOrigin::Current,
            )
            .unwrap();
        let code = Arc::downgrade(&module.program);
        drop(module);
        drop(runtime);
        assert!(code.upgrade().is_none());
        thread::spawn(move || drop(lease)).join().unwrap();
    }

    #[test]
    fn quarantine_blocks_state_access_retention_and_publication() {
        let mut runtime = Runtime::default();
        let (target, baseline) = load(&mut runtime);
        let candidate = runtime
            .stage_reload_program(&target, None, program())
            .unwrap();
        runtime
            .resources()
            .quarantine(EngineFault::HostInvariant("test execution fault"));
        assert!(runtime.module_instance_snapshot(&baseline).is_none());
        assert_eq!(
            runtime
                .write_module_slot(&baseline, ModuleSlot::new(0), &RootedValue::unit())
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        assert!(
            runtime
                .retain_module(
                    &baseline,
                    ModuleEpochRetention::ActiveCall,
                    RetentionOrigin::Current
                )
                .is_none()
        );
        assert!(runtime.publish_staged_reload(candidate).is_err());
        assert_eq!(
            load_bytecode(&mut runtime, "new", program())
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        assert_eq!(
            runtime
                .modules
                .capture_current(&target.installation)
                .unwrap()
                .key(),
            baseline.key()
        );
        assert_eq!(runtime.modules().loaded_count(), 1);
    }
}
