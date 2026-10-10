//! Program-owned applied executable descriptors with bounded retention.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge, application_key::ApplicationKey, applications::ApplicationId,
    },
    module::{LoadedModule, ModuleStore, ModuleStoreInner},
};
use std::collections::{HashMap, VecDeque};

// Bound polymorphic preparation retained by each linked module. Eviction drops
// only the program edge; active frames/host handles retain their own identities.
const RETAINED_APPLICATIONS: usize = 128;

#[derive(Debug, Default)]
pub(super) struct ApplicationCache {
    entries: HashMap<ApplicationKey, PublishedApplication>,
    order: VecDeque<ApplicationKey>,
}

#[derive(Debug)]
pub(super) struct PublishedApplication {
    pub(super) id: ApplicationId,
    dependencies: Vec<LoadedModule>,
}

impl PublishedApplication {
    pub(super) fn is_available(&self, store: &ModuleStoreInner) -> bool {
        self.dependencies
            .iter()
            .all(|owner| store.resolve(owner).is_some())
    }
}

impl ApplicationCache {
    pub(super) fn values(&self) -> impl Iterator<Item = &PublishedApplication> {
        self.entries.values()
    }

    fn insert(
        &mut self,
        key: ApplicationKey,
        application: PublishedApplication,
    ) -> Result<(), RuntimeError> {
        if self.entries.contains_key(&key) {
            return Err(RuntimeError::module_validation(
                "duplicate executable application",
            ));
        }
        self.entries
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("application index"))?;
        self.order
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("application order"))?;
        if self.entries.len() == RETAINED_APPLICATIONS {
            let expired = self.order.pop_front().expect("bounded application order");
            self.entries.remove(&expired);
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, application);
        Ok(())
    }
}

impl ModuleStore {
    pub(crate) fn method_application(
        &self,
        owner: &LoadedModule,
        key: &ApplicationKey,
    ) -> Option<ApplicationId> {
        let records = self.inner.try_borrow().ok()?;
        let entry = records.resolve(owner)?.applications.entries.get(key)?;
        entry.is_available(&records).then_some(entry.id)
    }
}

impl Runtime {
    pub(crate) fn publish_method_application(
        &self,
        owner: &LoadedModule,
        key: ApplicationKey,
        prepared: ApplicationId,
    ) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        self.validate_metadata(MetadataEdge::Program(owner))?;
        let dependencies = self.metadata_dependencies(MetadataEdge::Application(prepared))?;
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation(
                "module store is borrowed during application publication",
            )
        })?;
        let record = records
            .resolve_mut(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid application owner"))?;
        record.applications.insert(
            key,
            PublishedApplication {
                id: prepared,
                dependencies,
            },
        )
    }
}
