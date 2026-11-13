//! Immutable group membership and indexes; selected witnesses keep their own visibility.
use crate::{
    error::RuntimeError,
    execution_metadata::{
        groups::{OperationGroupId, OperationId},
        operation::BoundOperation,
    },
    frame::types::{bindings::AssociatedInterface, compatibility::TypeIdentity},
    resource::ResourceState,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{declaration::requirement::NativeCallableRequirement, ty::NominalTy};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Default)]
struct ReceiverMethods {
    members: HashMap<DefinitionId, usize>,
}

type SlotIndex = HashMap<NominalTy, HashMap<Arc<TypeIdentity>, Vec<Option<usize>>>>;

#[derive(Debug)]
pub(crate) struct OperationGroup {
    id: OperationGroupId,
    entries: Vec<BoundOperation>,
    index: HashMap<NominalTy, HashMap<Arc<TypeIdentity>, ReceiverMethods>>,
    slots: SlotIndex,
}

impl OperationGroup {
    pub(crate) fn id(&self) -> OperationGroupId {
        self.id
    }

    pub(in crate::execution_metadata) fn entries(&self) -> &[BoundOperation] {
        &self.entries
    }

    pub(super) fn new(
        entries: Vec<BoundOperation>,
        id: OperationGroupId,
        resources: &ResourceState,
    ) -> Result<Self, RuntimeError> {
        let reserve = |_| RuntimeError::resource_limit("operation dictionary indexes");
        let mut index: HashMap<NominalTy, HashMap<Arc<TypeIdentity>, ReceiverMethods>> =
            HashMap::new();
        let mut slots = SlotIndex::new();
        for (position, operation) in entries.iter().enumerate() {
            if position.is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            index.try_reserve(1).map_err(reserve)?;
            let receivers = index
                .entry(operation.requirement_key.interface.clone())
                .or_default();
            receivers.try_reserve(1).map_err(reserve)?;
            let methods = receivers
                .entry(operation.receiver_identity.clone())
                .or_default();
            methods.members.try_reserve(1).map_err(reserve)?;
            let slot = usize::try_from(operation.slot)
                .map_err(|_| RuntimeError::resource_limit("operation dictionary slot index"))?;
            let end = slot
                .checked_add(1)
                .ok_or_else(|| RuntimeError::resource_limit("operation dictionary slot index"))?;
            slots.try_reserve(1).map_err(reserve)?;
            let receivers = slots.entry(operation.interface_key.clone()).or_default();
            receivers.try_reserve(1).map_err(reserve)?;
            let receiver_slots = receivers
                .entry(operation.receiver_identity.clone())
                .or_default();
            let growth = end.saturating_sub(receiver_slots.len());
            receiver_slots.try_reserve(growth).map_err(reserve)?;
            while receiver_slots.len() < end {
                resources.poll_closed()?;
                let chunk_end = end.min(receiver_slots.len().saturating_add(1024));
                receiver_slots.resize(chunk_end, None);
            }
            receiver_slots[slot].get_or_insert(position);
            methods
                .members
                .entry(operation.requirement_key.member)
                .or_insert(position);
        }
        Ok(Self {
            id,
            entries,
            index,
            slots,
        })
    }

    pub(crate) fn operation_ids(&self) -> impl ExactSizeIterator<Item = OperationId> + '_ {
        (0..self.entries.len()).map(|member| OperationId {
            group: self.id,
            member,
        })
    }

    pub(crate) fn get(&self, id: OperationId) -> Option<&BoundOperation> {
        (id.group == self.id)
            .then(|| self.entries.get(id.member))
            .flatten()
    }

    pub(crate) fn associated_interfaces(
        &self,
    ) -> impl ExactSizeIterator<Item = AssociatedInterface> + '_ {
        self.entries.iter().map(|operation| AssociatedInterface {
            lookup_scope: None,
            outputs: Default::default(),
            receiver: operation.requirement.receiver.clone(),
            interface: operation.associated_interface.clone(),
            owner: operation.owner.clone(),
        })
    }

    fn methods(&self, receiver: &TypeIdentity, interface: &NominalTy) -> Option<&ReceiverMethods> {
        self.index.get(interface)?.get(receiver)
    }

    pub(crate) fn slot(
        &self,
        receiver: &TypeIdentity,
        interface: &NominalTy,
        slot: u32,
    ) -> Option<OperationId> {
        let position = self
            .slots
            .get(interface)?
            .get(receiver)?
            .get(slot as usize)?
            .as_ref()?;
        self.entries.get(*position).map(|_| OperationId {
            group: self.id,
            member: *position,
        })
    }

    /// A checked selected table can provide more associated outputs than a
    /// caller requested. Return only an unambiguous member from that table.
    pub(crate) fn selected_method(
        &self,
        required: &NativeCallableRequirement,
        receiver: &TypeIdentity,
    ) -> Option<OperationId> {
        let mut matches = self.entries.iter().enumerate().filter(|(_, operation)| {
            operation.generic.is_some()
                && operation.receiver_identity.as_ref() == receiver
                && operation.requirement_key.member == required.member
                && operation
                    .requirement_key
                    .interface
                    .satisfies(&required.interface)
        });
        let (member, _) = matches.next()?;
        matches.next().is_none().then_some(OperationId {
            group: self.id,
            member,
        })
    }

    pub(crate) fn operation(
        &self,
        required: &NativeCallableRequirement,
        receiver: &TypeIdentity,
    ) -> Option<OperationId> {
        let position = self
            .methods(receiver, &required.interface)?
            .members
            .get(&required.member)?;
        self.entries
            .get(*position)
            .filter(|operation| operation.matches_requirement(required))
            .map(|_| OperationId {
                group: self.id,
                member: *position,
            })
    }
}
