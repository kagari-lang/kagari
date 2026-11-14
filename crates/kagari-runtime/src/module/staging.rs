//! Candidates own exact managed revisions; leases separately control admission.
mod managed;
use crate::{
    error::{RuntimeError, reason::StaleHandleReason},
    execution::{memory::ManagedMemory, roots::ExecutionRoots},
    module::{
        ExecutableModule, Installation, ModuleKey, ModuleStore,
        installation::CurrentPublication,
        managed::{InstallationState, revision::RevisionState, source::RevisionSource},
    },
    resource::ResourceState,
};

use kagari_gc::{error::GcError, root::Root};
use std::{
    collections::hash_map::Entry,
    mem,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

/// The store keeps only a Weak identity. Last release requests a collection
/// without borrowing storage or keeping the runtime/program graph alive.
#[derive(Debug)]
pub(super) struct CandidateLease {
    abandoned: Arc<AtomicBool>,
    published: AtomicBool,
    pub(super) initial_load: bool,
}

impl CandidateLease {
    pub(super) fn new(abandoned: Arc<AtomicBool>, initial_load: bool) -> Self {
        Self {
            abandoned,
            published: AtomicBool::new(false),
            initial_load,
        }
    }
}

impl Drop for CandidateLease {
    fn drop(&mut self) {
        if !self.published.load(Ordering::Relaxed) {
            self.abandoned.store(true, Ordering::Release);
        }
    }
}

/// Metadata preparation has an abandonment lease before any fallible heap work.
/// This private intermediate never reaches an execution/publication entrypoint.
#[derive(Debug)]
pub(crate) struct PreparedProgramMetadata {
    pub(super) installation: Arc<Installation>,
    pub(super) module: ExecutableModule,
    pub(super) lease: Arc<CandidateLease>,
    pub(super) expected_base: Option<ModuleKey>,
}

#[derive(Debug)]
pub(crate) enum ManagedProgramError {
    Validation(RuntimeError),
    Memory(GcError),
}

impl From<RuntimeError> for ManagedProgramError {
    fn from(error: RuntimeError) -> Self {
        Self::Validation(error)
    }
}

impl From<GcError> for ManagedProgramError {
    fn from(error: GcError) -> Self {
        Self::Memory(error)
    }
}

/// Owns one unpublished revision root, independently of metadata-only leases.
/// Last-drop invalidates candidate access; collection retires its physical records.
#[derive(Debug)]
pub(crate) struct StagedProgram {
    pub(super) revision: Root<RevisionState>,
    pub(super) installation: Arc<Installation>,
    pub(super) module: ExecutableModule,
    pub(super) lease: Arc<CandidateLease>,
    pub(super) expected_base: Option<ModuleKey>,
}

impl StagedProgram {
    pub(crate) fn module(&self) -> &ExecutableModule {
        &self.module
    }

    pub(crate) fn bind_reload(
        self,
        store: &ModuleStore,
        baseline: &ExecutableModule,
        memory: &mut ManagedMemory,
        resources: &ResourceState,
    ) -> Result<Self, ManagedProgramError> {
        if self.expected_base != Some(baseline.key())
            || self.module.installation_id() != baseline.installation_id()
        {
            return Err(RuntimeError::module_validation(
                "reload demand history belongs to a different baseline",
            )
            .into());
        }
        store.bind_static_reload(baseline, &self.module)?;
        self.finish_bindings(store, memory, resources)?;
        Ok(self)
    }

    pub(crate) fn publish(
        self,
        store: &ModuleStore,
        memory: &mut ManagedMemory,
        resources: &ResourceState,
    ) -> Result<ExecutableModule, ManagedProgramError> {
        store.validate_installation(&self.installation, self.module.registry_owner)?;
        let replacement = self.module.program.clone();
        let mut inner = store.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store is borrowed during publication")
        })?;
        let key = self.module.program_key();
        let valid = inner.resolve(&self.module).is_some()
            && inner
                .staged
                .get(&key)
                .is_some_and(|lease| Weak::ptr_eq(lease, &Arc::downgrade(&self.lease)));
        if !valid || self.module.installation_id() != self.installation.id {
            return Err(
                RuntimeError::module_validation("invalid staged program publication").into(),
            );
        }
        // All correspondence, table allocations and environment-view replacement
        // happen before visibility changes. Parked frames hold no active views.
        let next_routes = inner.routes.prepare(&self.module)?;
        let mut next_execution = inner
            .records
            .iter()
            .filter(|(_, record)| {
                record.module.installation_id() == self.module.installation_id()
                    && (!inner.staged.contains_key(&record.module.program_key())
                        || record.module.program_key() == key)
            })
            .filter_map(|(key, record)| {
                record.execution.as_ref().map(|execution| (*key, execution))
            })
            .map(|(key, execution)| Ok((key, Some(execution.rebound(&next_routes)?))))
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        // Resolve the hash-table entry before taking the cell lock. Removing this
        // occupied entry in the commit neither allocates nor hashes again.
        let entry = match inner.staged.entry(key) {
            Entry::Occupied(entry) => entry,
            Entry::Vacant(_) => {
                return Err(RuntimeError::module_validation("missing staged publication").into());
            }
        };
        let mut current = store.current.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("current programs borrowed during publication")
        })?;
        current
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("current programs"))?;
        let current_key = current.get(&self.installation.id).map(|publication| {
            let program = &publication.program;
            let root = &program.modules[program.root.index()];
            ModuleKey {
                id: root.id,
                epoch: root.epoch,
            }
        });
        if current_key != self.expected_base {
            return Err(RuntimeError::from(
                StaleHandleReason::InstallationPublicationBaselineChangedOrClosed,
            )
            .into());
        }
        // Prepare ownership before the physical commit. Compatible publication
        // shares the existing installation lease; a fresh load creates one root.
        let root = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let revision =
                    RevisionSource::Host(&self.revision).resolve(store, slots, &self.module)?;
                let installation = RevisionState::installation(slots.scope(), revision);
                let root = match current.get(&self.installation.id) {
                    Some(publication) => {
                        let existing = slots.scope().get(&publication.root)?;
                        if !existing.ptr_eq(installation) {
                            return Err(GcError::Representation);
                        }
                        publication.root.clone()
                    }
                    None => slots.scope().retain(installation)?,
                };
                InstallationState::publish(slots.scope(), installation, revision)?;
                Ok(root)
            })
        })?;
        let replacement = CurrentPublication {
            program: replacement,
            root,
        };
        // Everything below is non-failing; no destruction of the old descriptor
        // occurs until both guards have been released.
        self.module.program.dispatch.scope.mark_published();
        self.lease.published.store(true, Ordering::Relaxed);
        let removed_stage = entry.remove();
        let previous = current.insert(self.installation.id, replacement);
        let previous_routes = mem::replace(&mut inner.routes, next_routes);
        for (key, execution) in &mut next_execution {
            mem::swap(
                &mut inner
                    .records
                    .get_mut(key)
                    .expect("prepared execution owner")
                    .execution,
                execution,
            );
        }
        drop(current);
        drop(inner);
        drop(previous_routes);
        drop(next_execution);
        drop(previous);
        drop(removed_stage);
        Ok(self.module.clone())
    }
}

#[cfg(test)]
mod tests;
