//! Weak table facts never retain receivers or mutable executable backing.
use crate::{
    frame::types::{
        compatibility::TypeIdentity, construction::ConstructionIdentity,
        operations::OperationSegment,
    },
    module::ModuleKey,
    objects::interfaces::description::InterfaceDescription,
};
use std::{
    collections::{HashMap, TryReserveError},
    sync::{Arc, Weak},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DescriptionKey {
    pub(crate) owner: ModuleKey,
    pub(crate) table: usize,
    pub(crate) view: bool,
    pub(crate) arguments: Vec<Arc<TypeIdentity>>,
    pub(crate) constructions: ConstructionIdentity,
    pub(crate) operations: Option<Arc<Vec<OperationSegment>>>,
}

#[derive(Debug, Default)]
pub(crate) struct DescriptionCache(HashMap<DescriptionKey, Weak<InterfaceDescription>>);

impl DescriptionCache {
    pub(crate) fn get(&self, key: &DescriptionKey) -> Option<Arc<InterfaceDescription>> {
        self.0.get(key)?.upgrade()
    }

    pub(crate) fn insert(
        &mut self,
        key: DescriptionKey,
        description: &Arc<InterfaceDescription>,
    ) -> Result<(), TryReserveError> {
        self.0.try_reserve(1)?;
        self.0.insert(key, Arc::downgrade(description));
        Ok(())
    }

    pub(crate) fn prune(&mut self) {
        self.0
            .retain(|_, description| description.strong_count() != 0);
    }
}
