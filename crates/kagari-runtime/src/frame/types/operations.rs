//! Shared receiver selections and call-local witnesses retain their supplying group.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        groups::{OperationGroupId, OperationId},
    },
    frame::types::{bindings::AssociatedInterface, compatibility::TypeIdentity},
    gc::GcHeap,
    module::dispatch::SelectedTarget,
};
use kagari_types::{declaration::requirement::NativeCallableRequirement, ty::NominalTy};
use std::sync::Arc;

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
    ) -> Result<Self, RuntimeError> {
        let invalid = || RuntimeError::module_validation("transported operation witness");
        let mut ids = Vec::new();
        for segment in self.segments() {
            match segment {
                OperationSegment::Selected(id) => ids.push(*id),
                OperationSegment::Transported(key) => ids.push(key.id),
                OperationSegment::Receiver(id) => ids.extend(
                    runtime
                        .gc
                        .operation_group(*id)
                        .ok_or_else(invalid)?
                        .operation_ids(),
                ),
            }
        }
        let mut result = Self::default();
        for id in ids {
            let operation = runtime.gc.bound_operation(id).ok_or_else(invalid)?;
            if operation.owner.installation_id() != selected.owner().installation_id() {
                if !operation
                    .owner
                    .is_shared_native_trait(operation.requirement.interface.declaration)
                {
                    return Err(RuntimeError::module_validation(
                        "script trait witness belongs to another installation",
                    ));
                }
                result.push(&runtime.gc, id)?;
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
                    .remap(&operation.requirement, &Default::default())?
                    .into_records(),
                interface: mapping
                    .remap(&operation.associated_interface, &Default::default())?
                    .into_records(),
                slot: operation.slot,
                generic: operation.generic.is_some(),
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
    pub(crate) fn validate(&self, heap: &GcHeap) -> bool {
        self.segments().iter().all(|segment| match segment {
            OperationSegment::Selected(id) => heap.bound_operation(*id).is_some(),
            OperationSegment::Receiver(id) => heap.operation_group(*id).is_some(),
            OperationSegment::Transported(key) => heap.bound_operation(key.id).is_some(),
        })
    }

    pub(crate) fn trace_metadata<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.extend(self.segments().iter().map(|segment| match segment {
            OperationSegment::Selected(operation) => MetadataEdge::Operation(*operation),
            OperationSegment::Receiver(group) => MetadataEdge::Group(*group),
            OperationSegment::Transported(key) => MetadataEdge::Operation(key.id),
        }));
    }

    pub(crate) fn associated_interfaces(&self) -> Arc<Vec<AssociatedInterface>> {
        self.associated.clone().unwrap_or_default()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.segments().is_empty()
    }

    pub(crate) fn push(&mut self, heap: &GcHeap, id: OperationId) -> Result<(), RuntimeError> {
        let operation = heap
            .bound_operation(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid selected operation"))?;
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

    pub(crate) fn receiver(
        &mut self,
        heap: &GcHeap,
        id: OperationGroupId,
    ) -> Result<(), RuntimeError> {
        let group = heap
            .operation_group(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid receiver group"))?;
        Arc::make_mut(self.associated.get_or_insert_default())
            .extend(group.associated_interfaces());
        Arc::make_mut(self.segments.get_or_insert_default()).push(OperationSegment::Receiver(id));
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
        heap: &GcHeap,
        receiver: &TypeIdentity,
        interface: &NominalTy,
        slot: u32,
    ) -> Option<OperationId> {
        self.segments().iter().find_map(|segment| match segment {
            OperationSegment::Selected(id) => {
                let operation = heap.bound_operation(*id)?;
                (operation.slot == slot
                    && operation.receiver_identity.as_ref() == receiver
                    && operation.associated_interface.satisfies(interface))
                .then_some(*id)
            }
            OperationSegment::Receiver(id) => {
                heap.operation_group(*id)?.slot(receiver, interface, slot)
            }
            OperationSegment::Transported(key) => (key.slot == slot
                && key.receiver.same_contract(receiver)
                && key.interface.satisfies(interface))
            .then_some(key.id),
        })
    }

    pub(crate) fn operation(
        &self,
        heap: &GcHeap,
        required: &NativeCallableRequirement,
        receiver: &TypeIdentity,
    ) -> Option<OperationId> {
        self.segments().iter().find_map(|segment| match segment {
            OperationSegment::Selected(id) => {
                let operation = heap.bound_operation(*id)?;
                (operation.receiver_identity.as_ref() == receiver
                    && operation.matches_requirement(required))
                .then_some(*id)
            }
            OperationSegment::Receiver(id) => {
                heap.operation_group(*id)?.operation(required, receiver)
            }
            OperationSegment::Transported(key) => (key.receiver.same_contract(receiver)
                && key.requirement.receiver == required.receiver
                && key.requirement.member == required.member
                && key.interface.satisfies(&required.interface)
                && (key.generic || key.requirement.arguments == required.arguments))
                .then_some(key.id),
        })
    }
}
