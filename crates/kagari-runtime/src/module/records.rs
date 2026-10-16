//! Runtime-local links and caches are owned with the installed module instance.
use crate::{
    module::{
        LoadedModule, ModuleInstance, ModuleKey, ModuleStore, ModuleStoreInner,
        constants::ConstantPool,
        descriptors::LinkedDescriptors,
        layouts::{LayoutCache, ProgramLayoutCache},
        linked_execution::LinkedExecution,
        staging::CandidateLease,
    },
    native::binding::LinkedNativeFunction,
};
use kagari_bytecode::instruction::NativeImportId;
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

#[derive(Debug)]
pub(super) struct ModuleRecord {
    pub(super) module: LoadedModule,
    pub(super) instance: ModuleInstance,
    pub(super) layouts: LayoutCache,
    pub(super) constants: Arc<ConstantPool>,
    pub(super) descriptors: LinkedDescriptors,
    pub(super) execution: Option<LinkedExecution>,
    native: Vec<Arc<LinkedNativeFunction>>,
    program_layouts: Option<Box<ProgramLayoutCache>>,
}

impl ModuleRecord {
    pub(super) fn new(module: LoadedModule, native: Vec<Arc<LinkedNativeFunction>>) -> Self {
        Self {
            instance: ModuleInstance::new(&module),
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

    pub(super) fn matches(&self, module: &LoadedModule) -> bool {
        self.module.slot == module.slot && Arc::ptr_eq(&self.module.program, &module.program)
    }
}

impl ModuleStoreInner {
    pub(super) fn available(&self, key: ModuleKey) -> Option<&LoadedModule> {
        self.available_record(key).map(|record| &record.module)
    }

    fn available_record(&self, key: ModuleKey) -> Option<&ModuleRecord> {
        let record = self.records.get(&key)?;
        candidate_available(&self.staged, &record.module).then_some(record)
    }

    pub(super) fn resolve(&self, module: &LoadedModule) -> Option<&ModuleRecord> {
        self.available_record(module.key())
            .filter(|record| record.matches(module))
    }

    pub(super) fn resolve_mut(&mut self, module: &LoadedModule) -> Option<&mut ModuleRecord> {
        let record = self.records.get_mut(&module.key())?;
        (candidate_available(&self.staged, &record.module) && record.matches(module))
            .then_some(record)
    }
}

/// Availability belongs to the installed record, including abandoned candidates
/// whose immutable descriptors still exist outside the runtime.
fn candidate_available(
    staged: &HashMap<ModuleKey, Weak<CandidateLease>>,
    module: &LoadedModule,
) -> bool {
    staged
        .get(&module.program_key())
        .is_none_or(|lease| lease.strong_count() != 0)
}

impl ModuleStore {
    /// Clone only for the synchronous invocation; no module-store borrow crosses Rust code.
    pub(crate) fn native_binding(
        &self,
        module: &LoadedModule,
        import: NativeImportId,
    ) -> Option<Arc<LinkedNativeFunction>> {
        let records = self.inner.try_borrow().ok()?;
        records.resolve(module)?.native.get(import.index()).cloned()
    }
}

#[cfg(test)]
mod tests;
