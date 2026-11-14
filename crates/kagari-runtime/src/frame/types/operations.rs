//! Shared receiver selections and call-local witnesses retain their supplying group.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        groups::{OperationGroupId, OperationId, group::OperationGroup},
        operation::BoundOperation,
    },
    frame::types::{bindings::AssociatedInterface, compatibility::TypeIdentity},
    module::dispatch::SelectedTarget,
};
use kagari_types::{declaration::requirement::NativeCallableRequirement, ty::NominalTy};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum OperationSegment {
    Selected(OperationId),
    Receiver(OperationGroupId),
    Transported(Arc<TransportedOperation>),
}

/// A selected-entry lookup key pointing back to the original checked witness.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TransportedOperation {
    id: OperationId,
    receiver: Arc<TypeIdentity>,
    requirement: NativeCallableRequirement,
    interface: NominalTy,
    interface_key: NominalTy,
    slot: u32,
    generic: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OperationBindings {
    segments: Option<Arc<Vec<OperationSegment>>>,
    // Type-only facts remain available without borrowing executable group storage.
    associated: Option<Arc<Vec<AssociatedInterface>>>,
}

impl OperationBindings {
    pub(crate) fn transported(
        &self,
        runtime: &Runtime,
        selected: &SelectedTarget,
        groups: &HashMap<OperationGroupId, Arc<OperationGroup>>,
    ) -> Result<Self, RuntimeError> {
        let invalid = || RuntimeError::module_validation("transported operation witness");
        let mut ids = Vec::new();
        for (index, segment) in self.segments().iter().enumerate() {
            if index.is_multiple_of(1024) {
                runtime.resources().poll_execution()?;
            }
            ids.try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("transported operation members"))?;
            match segment {
                OperationSegment::Selected(id) => ids.push(*id),
                OperationSegment::Transported(key) => ids.push(key.id),
                OperationSegment::Receiver(id) => {
                    let group = groups.get(id).ok_or_else(invalid)?;
                    ids.try_reserve(group.operation_ids().len()).map_err(|_| {
                        RuntimeError::resource_limit("transported operation members")
                    })?;
                    for (index, member) in group.operation_ids().enumerate() {
                        if index.is_multiple_of(1024) {
                            runtime.resources().poll_execution()?;
                        }
                        ids.push(member);
                    }
                }
            }
        }
        let mut result = Self::default();
        for (index, id) in ids.into_iter().enumerate() {
            if index.is_multiple_of(1024) {
                runtime.resources().poll_execution()?;
            }
            let group = groups.get(&id.group).ok_or_else(invalid)?;
            let operation = group.get(id).ok_or_else(invalid)?;
            if operation.owner.installation_id() != selected.owner().installation_id() {
                if !operation
                    .owner
                    .is_shared_native_trait(operation.requirement.interface.declaration)
                {
                    return Err(RuntimeError::module_validation(
                        "script trait witness belongs to another installation",
                    ));
                }
                result.append(operation, id)?;
                continue;
            }
            let mapping = selected.mapping_from(operation.owner.publication_definitions())?;
            let fact = AssociatedInterface {
                lookup_scope: None,
                outputs: Default::default(),
                receiver: operation.requirement.receiver.clone(),
                interface: operation.associated_interface.clone(),
                owner: operation.owner.clone(),
            }
            .transported(runtime, selected)?;
            let key = TransportedOperation {
                id,
                receiver: Arc::new(operation.receiver_identity.transported(&mapping)?),
                requirement: mapping
                    .remap_closed(&operation.requirement_key, &Default::default())?,
                interface: mapping.remap_closed(&operation.interface_key, &Default::default())?,
                slot: operation.slot,
                generic: operation.generic.is_some(),
                interface_key: mapping
                    .remap_closed(&operation.interface_key, &Default::default())?,
            };
            Arc::make_mut(result.segments.get_or_insert_default())
                .push(OperationSegment::Transported(Arc::new(key)));
            Arc::make_mut(result.associated.get_or_insert_default()).push(fact);
        }
        Ok(result)
    }

    pub(crate) fn identity(&self) -> Option<Arc<Vec<OperationSegment>>> {
        self.segments.clone()
    }

    fn segments(&self) -> &[OperationSegment] {
        self.segments
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn validate(&self, groups: &HashMap<OperationGroupId, Arc<OperationGroup>>) -> bool {
        self.segments().iter().all(|segment| match segment {
            OperationSegment::Selected(id) => groups
                .get(&id.group)
                .and_then(|group| group.get(*id))
                .is_some(),
            OperationSegment::Receiver(id) => groups.contains_key(id),
            OperationSegment::Transported(key) => groups
                .get(&key.id.group)
                .and_then(|group| group.get(key.id))
                .is_some(),
        })
    }

    pub(crate) fn trace_metadata<'a, E, F>(&'a self, visit: &mut F) -> Result<(), E>
    where
        F: FnMut(MetadataEdge<'a>) -> Result<(), E> + ?Sized,
    {
        for segment in self.segments() {
            visit(match segment {
                OperationSegment::Selected(operation) => MetadataEdge::Operation(*operation),
                OperationSegment::Receiver(group) => MetadataEdge::Group(*group),
                OperationSegment::Transported(key) => MetadataEdge::Operation(key.id),
            })?;
        }
        Ok(())
    }

    pub(crate) fn associated_interfaces(&self) -> Arc<Vec<AssociatedInterface>> {
        self.associated.clone().unwrap_or_default()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.segments().is_empty()
    }

    pub(crate) fn push(
        &mut self,
        group: &OperationGroup,
        id: OperationId,
    ) -> Result<(), RuntimeError> {
        let operation = group
            .get(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid selected operation"))?;
        self.append(operation, id)
    }

    fn append(&mut self, operation: &BoundOperation, id: OperationId) -> Result<(), RuntimeError> {
        Arc::make_mut(self.segments.get_or_insert_default())
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("operation bindings"))?;
        Arc::make_mut(self.associated.get_or_insert_default())
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("operation associated types"))?;
        Arc::make_mut(self.associated.get_or_insert_default()).push(AssociatedInterface {
            lookup_scope: None,
            outputs: Default::default(),
            receiver: operation.requirement.receiver.clone(),
            interface: operation.associated_interface.clone(),
            owner: operation.owner.clone(),
        });
        Arc::make_mut(self.segments.get_or_insert_default()).push(OperationSegment::Selected(id));
        Ok(())
    }

    pub(crate) fn receiver(&mut self, group: &OperationGroup) -> Result<(), RuntimeError> {
        Arc::make_mut(self.segments.get_or_insert_default())
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("operation bindings"))?;
        Arc::make_mut(self.associated.get_or_insert_default())
            .try_reserve(group.associated_interfaces().len())
            .map_err(|_| RuntimeError::resource_limit("operation associated types"))?;
        Arc::make_mut(self.associated.get_or_insert_default())
            .extend(group.associated_interfaces());
        Arc::make_mut(self.segments.get_or_insert_default())
            .push(OperationSegment::Receiver(group.id()));
        Ok(())
    }

    pub(crate) fn extend(&mut self, other: Self) {
        if other.is_empty() {
            return;
        }
        if self.is_empty() {
            *self = other;
            return;
        }
        Arc::make_mut(self.segments.get_or_insert_default())
            .extend(other.segments().iter().cloned());
        Arc::make_mut(self.associated.get_or_insert_default())
            .extend(other.associated_interfaces().iter().cloned());
    }

    pub(crate) fn operation_slot(
        &self,
        groups: &HashMap<OperationGroupId, Arc<OperationGroup>>,
        receiver: &TypeIdentity,
        interface: &NominalTy,
        slot: u32,
    ) -> Option<OperationId> {
        self.segments().iter().find_map(|segment| match segment {
            OperationSegment::Selected(id) => {
                let operation = groups.get(&id.group)?.get(*id)?;
                (operation.slot == slot
                    && operation.receiver_identity.as_ref() == receiver
                    && operation.interface_key.satisfies(interface))
                .then_some(*id)
            }
            OperationSegment::Receiver(id) => groups.get(id)?.slot(receiver, interface, slot),
            OperationSegment::Transported(key) => (key.slot == slot
                && key.receiver.same_contract(receiver)
                && key.interface_key.satisfies(interface))
            .then_some(key.id),
        })
    }

    pub(crate) fn operation(
        &self,
        groups: &HashMap<OperationGroupId, Arc<OperationGroup>>,
        required: &NativeCallableRequirement,
        receiver: &TypeIdentity,
    ) -> Option<OperationId> {
        self.segments().iter().find_map(|segment| match segment {
            OperationSegment::Selected(id) => {
                let operation = groups.get(&id.group)?.get(*id)?;
                (operation.receiver_identity.as_ref() == receiver
                    && operation.matches_requirement(required))
                .then_some(*id)
            }
            OperationSegment::Receiver(id) => groups.get(id)?.operation(required, receiver),
            OperationSegment::Transported(key) => (key.receiver.same_contract(receiver)
                && key.requirement.receiver == required.receiver
                && key.requirement.member == required.member
                && key.interface.satisfies(&required.interface)
                && (key.generic || key.requirement.arguments == required.arguments))
                .then_some(key.id),
        })
    }
}
