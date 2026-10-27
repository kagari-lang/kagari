//! Runtime-local links and caches are owned with the installed module instance.
pub(in crate::module) mod storage;
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::environments::EnvironmentId,
    module::{
        ExecutableModule, InstallationId, ModuleKey, ModuleStore, ModuleStoreInner,
        constants::ConstantPool,
        descriptors::LinkedDescriptors,
        layouts::{LayoutCache, ProgramLayoutCache},
        linked_execution::{LinkedExecution, LinkedFunction},
        records::storage::ResolvedRecord,
        staging::CandidateLease,
        state::cells::CellId,
    },
    native::binding::LinkedNativeFunction,
};
use kagari_bytecode::instruction::NativeImportId;
use kagari_contract::ids::FunctionRef;
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

#[derive(Debug)]
pub(super) struct ModuleRecord {
    pub(super) module: ExecutableModule,
    pub(super) bindings: Vec<CellId>,
    pub(super) static_lineage: InstallationId,
    pub(super) layouts: LayoutCache,
    pub(super) constants: Arc<ConstantPool>,
    pub(super) descriptors: LinkedDescriptors,
    pub(super) execution: Option<LinkedExecution>,
    native: Vec<Arc<LinkedNativeFunction>>,
    program_layouts: Option<Box<ProgramLayoutCache>>,
}

impl ModuleRecord {
    pub(super) fn new(module: ExecutableModule, native: Vec<Arc<LinkedNativeFunction>>) -> Self {
        Self {
            bindings: Vec::new(),
            static_lineage: module.installation_id(),
            constants: Arc::new(ConstantPool::new(module.bytecode.constants.len())),
            layouts: LayoutCache::default(),
            program_layouts: None,
            module,
            native,
            descriptors: LinkedDescriptors::default(),
            execution: None,
        }
    }

    pub(super) fn program_layouts(&mut self) -> Option<&mut ProgramLayoutCache> {
        if self.module.slot != self.module.program.root {
            return None;
        }
        Some(
            self.program_layouts.get_or_insert_with(|| {
                Box::new(ProgramLayoutCache::new(&self.module.program.layouts))
            }),
        )
    }

    pub(super) fn matches(&self, module: &ExecutableModule) -> bool {
        self.module.slot == module.slot && Arc::ptr_eq(&self.module.program, &module.program)
    }
}

impl ModuleStoreInner {
    pub(super) fn available(&self, key: ModuleKey) -> Option<&ExecutableModule> {
        self.available_record(key)
            .map(|resolved| &resolved.record().module)
    }

    fn available_record(&self, key: ModuleKey) -> Option<ResolvedRecord<'_>> {
        let resolved = self.records.resolve(&key)?;
        self.resolved_available(resolved).then_some(resolved)
    }

    pub(super) fn resolved_available(&self, resolved: ResolvedRecord<'_>) -> bool {
        self.records
            .accessible(resolved, &self.graph)
            .unwrap_or(false)
            && candidate_available(&self.staged, &resolved.record().module)
    }

    pub(super) fn resolve(&self, module: &ExecutableModule) -> Option<&ModuleRecord> {
        self.resolve_owner(module).map(ResolvedRecord::record)
    }

    pub(super) fn resolve_owner(&self, module: &ExecutableModule) -> Option<ResolvedRecord<'_>> {
        self.available_record(module.key())
            .filter(|resolved| resolved.record().matches(module))
    }

    pub(super) fn resolve_mut(&mut self, module: &ExecutableModule) -> Option<&mut ModuleRecord> {
        self.resolve(module)?;
        let record = self.records.get_mut(&module.key())?;
        (candidate_available(&self.staged, &record.module) && record.matches(module))
            .then_some(record)
    }
}

/// Availability belongs to the installed record, including abandoned candidates
/// whose immutable descriptors still exist outside the runtime.
fn candidate_available(
    staged: &HashMap<ModuleKey, Weak<CandidateLease>>,
    module: &ExecutableModule,
) -> bool {
    staged
        .get(&module.program_key())
        .is_none_or(|lease| lease.strong_count() != 0)
}

impl ModuleStore {
    /// Clone only for the synchronous invocation; no module-store borrow crosses Rust code.
    pub(crate) fn native_binding(
        &self,
        module: &ExecutableModule,
        import: NativeImportId,
    ) -> Option<Arc<LinkedNativeFunction>> {
        let records = self.inner.try_borrow().ok()?;
        records.resolve(module)?.native.get(import.index()).cloned()
    }
}

/// Executable admission retained only inside one non-reentrant preparation scope.
/// The original store borrow prevents record retirement/replacement; its candidate
/// lease is pinned by the scope. Publication still observes the current graph phase.
pub(crate) struct AdmittedExecutable<'a> {
    runtime: &'a Runtime,
    store: &'a ModuleStoreInner,
    resolved: ResolvedRecord<'a>,
}

impl AdmittedExecutable<'_> {
    pub(crate) fn runtime(&self) -> &Runtime {
        self.runtime
    }

    pub(crate) fn module(&self) -> &ExecutableModule {
        &self.resolved.record().module
    }

    pub(crate) fn linked_function(
        &self,
        function: FunctionRef,
        environment: Option<EnvironmentId>,
    ) -> Option<Arc<LinkedFunction>> {
        self.resolved
            .record()
            .execution
            .as_ref()?
            .function(function, environment)
    }

    pub(crate) fn publish(&self) -> Result<(), RuntimeError> {
        self.store.publish_resolved_program(self.resolved)
    }
}

impl Runtime {
    /// Keep admission at its original error-order boundary, then consume the same
    /// owner through link preparation, reservations and frame publication. The
    /// callback is internal preparation only: no user callbacks or GC steps.
    pub(crate) fn with_frame_admission<R>(
        &self,
        module: &ExecutableModule,
        prepare: impl FnOnce(AdmittedExecutable<'_>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        self.with_executable_owner(module, |store, resolved| {
            let _candidate = store
                .staged
                .get(&module.program_key())
                .map(|lease| {
                    lease.upgrade().ok_or_else(|| {
                        RuntimeError::new(
                            RuntimeErrorKind::StaleHandle,
                            "executable module has been released",
                        )
                    })
                })
                .transpose()?;
            prepare(AdmittedExecutable {
                runtime: self,
                store,
                resolved,
            })
        })
    }

    /// Validate exact runtime, availability and installation before execution.
    pub fn validate_executable_module(
        &self,
        module: &ExecutableModule,
    ) -> Result<(), RuntimeError> {
        self.with_executable_owner(module, |_, _| Ok(()))
    }

    /// Validate and publish under one module-store borrow. No admitted-owner proof
    /// escapes to a later callback, collection, candidate release or reload boundary.
    pub(crate) fn publish_executable_module(
        &self,
        module: &ExecutableModule,
    ) -> Result<(), RuntimeError> {
        self.with_executable_owner(module, |store, resolved| {
            store.publish_resolved_program(resolved)
        })
    }

    fn with_executable_owner<R>(
        &self,
        module: &ExecutableModule,
        action: impl FnOnce(&ModuleStoreInner, ResolvedRecord<'_>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        self.resources().ensure_execution_allowed()?;
        if !module.belongs_to(self.host.owner()) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::StaleHandle,
                "executable module belongs to another runtime",
            ));
        }
        let store = self.modules.inner.try_borrow().map_err(|_| {
            self.resources()
                .quarantine("module store is borrowed across execution")
        })?;
        let resolved = store.resolve_owner(module).ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::StaleHandle,
                "executable module has been released",
            )
        })?;
        self.modules.validate_executable_installation(module)?;
        action(&store, resolved)
    }
}

#[cfg(test)]
mod tests;
