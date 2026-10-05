//! Runtime-local links and caches are owned with the installed module instance.
use crate::{
    module::{LoadedModule, ModuleInstance, ModuleStore, ModuleStoreInner, layouts::LayoutCache},
    native::binding::LinkedNativeFunction,
};
use kagari_bytecode::instruction::NativeImportId;
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct ModuleRecord {
    pub(super) module: LoadedModule,
    pub(super) instance: ModuleInstance,
    pub(super) layouts: LayoutCache,
    native: Vec<Arc<LinkedNativeFunction>>,
}

impl ModuleRecord {
    pub(super) fn new(module: LoadedModule, native: Vec<Arc<LinkedNativeFunction>>) -> Self {
        Self {
            instance: ModuleInstance::new(&module),
            module,
            native,
            layouts: LayoutCache::default(),
        }
    }

    pub(super) fn matches(&self, module: &LoadedModule) -> bool {
        self.module.slot == module.slot && Arc::ptr_eq(&self.module.program, &module.program)
    }
}

impl ModuleStoreInner {
    pub(super) fn resolve(&self, module: &LoadedModule) -> Option<&ModuleRecord> {
        self.available(module.key())?;
        self.records
            .get(&module.key())
            .filter(|record| record.matches(module))
    }

    pub(super) fn resolve_mut(&mut self, module: &LoadedModule) -> Option<&mut ModuleRecord> {
        self.available(module.key())?;
        self.records
            .get_mut(&module.key())
            .filter(|record| record.matches(module))
    }
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
