//! Shared receiver selections and call-local witnesses retain their supplying group.
use crate::{
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        groups::{OperationGroupId, OperationId},
    },
    frame::types::bindings::AssociatedInterface,
    gc::GcHeap,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{NominalTy, Ty},
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum OperationSegment {
    Selected(OperationId),
    Receiver(OperationGroupId),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OperationBindings {
    segments: Option<Arc<Vec<OperationSegment>>>,
    // Type-only facts remain available without borrowing executable group storage.
    associated: Option<Arc<Vec<AssociatedInterface>>>,
}

impl OperationBindings {
    pub(crate) fn identity(&self) -> Option<Arc<Vec<OperationSegment>>> {
        self.segments.clone()
    }

    fn segments(&self) -> &[OperationSegment] {
        self.segments
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn validate(&self, heap: &GcHeap) -> bool {
        self.segments().iter().all(|segment| match segment {
            OperationSegment::Selected(id) => heap.bound_operation(*id).is_some(),
            OperationSegment::Receiver(id) => heap.operation_group(*id).is_some(),
        })
    }

    pub(crate) fn trace_metadata<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.extend(self.segments().iter().map(|segment| match segment {
            OperationSegment::Selected(operation) => MetadataEdge::Operation(*operation),
            OperationSegment::Receiver(group) => MetadataEdge::Group(*group),
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
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
        slot: u32,
    ) -> Option<OperationId> {
        self.segments().iter().find_map(|segment| match segment {
            OperationSegment::Selected(id) => {
                let operation = heap.bound_operation(*id)?;
                (operation.slot == slot
                    && operation.requirement.receiver == *receiver
                    && operation.requirement.interface == *interface)
                    .then_some(*id)
            }
            OperationSegment::Receiver(id) => {
                heap.operation_group(*id)?.slot(receiver, interface, slot)
            }
        })
    }

    pub(crate) fn operation(
        &self,
        heap: &GcHeap,
        required: &NativeCallableRequirement<DefinitionId>,
    ) -> Option<OperationId> {
        self.segments().iter().find_map(|segment| match segment {
            OperationSegment::Selected(id) => {
                let operation = heap.bound_operation(*id)?;
                operation.matches_requirement(required).then_some(*id)
            }
            OperationSegment::Receiver(id) => heap.operation_group(*id)?.operation(required),
        })
    }
}
