//! Runtime-owned method applications, addressed by checked identities.
use crate::{
    error::RuntimeError,
    frame::types::{TypeEnvironment, arguments::ScopedSignature},
    gc::{GcHeap, interfaces::InterfaceResultBinding},
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::callable::Signature;
use std::{
    cell::Ref,
    collections::{HashSet, TryReserveError},
};

#[derive(Debug)]
pub(crate) struct MethodApplication {
    pub(crate) signature: Signature<DefinitionId>,
    pub(crate) scoped_signature: Option<ScopedSignature>,
    pub(crate) environment: Option<TypeEnvironment>,
    pub(crate) result_adapter: Option<InterfaceResultBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ApplicationId {
    owner: u64,
    slot: usize,
    generation: u64,
}

#[derive(Debug)]
struct Slot {
    generation: u64,
    application: Option<MethodApplication>,
}

#[derive(Debug)]
pub(crate) struct ApplicationStore {
    owner: u64,
    slots: Vec<Slot>,
    free: Vec<usize>,
    count: usize,
}

impl ApplicationStore {
    pub(crate) fn new(owner: u64) -> Self {
        Self {
            owner,
            slots: Vec::new(),
            free: Vec::new(),
            count: 0,
        }
    }
    pub(crate) fn insert(
        &mut self,
        application: MethodApplication,
    ) -> Result<ApplicationId, TryReserveError> {
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.slots.try_reserve(1)?;
                self.slots.push(Slot {
                    generation: 0,
                    application: None,
                });
                self.slots.len() - 1
            }
        };
        let record = &mut self.slots[slot];
        let id = ApplicationId {
            owner: self.owner,
            slot,
            generation: record.generation,
        };
        record.application = Some(application);
        self.count += 1;
        Ok(id)
    }

    pub(crate) fn get(&self, id: ApplicationId) -> Option<&MethodApplication> {
        if id.owner != self.owner {
            return None;
        }
        let slot = self.slots.get(id.slot)?;
        (slot.generation == id.generation)
            .then_some(slot.application.as_ref())
            .flatten()
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// The whole graph has been marked and validated before any store detaches.
    pub(crate) fn detach(&mut self, live: &HashSet<ApplicationId>) -> Vec<MethodApplication> {
        let mut retired = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let id = ApplicationId {
                owner: self.owner,
                slot: index,
                generation: slot.generation,
            };
            if !live.contains(&id)
                && let Some(application) = slot.application.take()
            {
                retired.push(application);
                self.count -= 1;
                if let Some(generation) = slot.generation.checked_add(1) {
                    slot.generation = generation;
                    self.free.push(index);
                }
            }
        }
        retired
    }
}

impl GcHeap {
    pub(crate) fn alloc_method_application(
        &self,
        application: MethodApplication,
    ) -> Result<ApplicationId, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.applications
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("method application store is borrowed"))?
            .insert(application)
            .map_err(|_| self.resource_limit("method application storage"))
    }

    pub(crate) fn method_application(
        &self,
        id: ApplicationId,
    ) -> Option<Ref<'_, MethodApplication>> {
        Ref::filter_map(self.applications.try_borrow().ok()?, |store| store.get(id)).ok()
    }
}

#[cfg(test)]
mod tests;
