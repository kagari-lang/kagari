//! Immutable group membership and indexes; selected witnesses keep their own visibility.
use crate::{
    execution_metadata::{
        MetadataEdge,
        groups::{OperationGroupId, OperationId},
        operation::BoundOperation,
    },
    frame::types::{bindings::AssociatedInterface, compatibility::TypeIdentity},
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{declaration::requirement::NativeCallableRequirement, ty::NominalTy};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Default)]
struct ReceiverMethods {
    slots: Vec<Option<usize>>,
    members: HashMap<DefinitionId, usize>,
}

#[derive(Debug)]
pub(crate) struct OperationGroup {
    id: OperationGroupId,
    entries: Vec<BoundOperation>,
    index: HashMap<NominalTy, HashMap<Arc<TypeIdentity>, ReceiverMethods>>,
}

impl OperationGroup {
    pub(crate) fn scan_len(&self) -> usize {
        self.entries.len() * 5
    }

    pub(crate) fn scan_field(&self, index: usize) -> Option<MetadataEdge<'_>> {
        self.entries.get(index / 5)?.scan_field(index % 5)
    }

    pub(crate) fn trace_metadata<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.extend((0..self.scan_len()).filter_map(|field| self.scan_field(field)));
    }

    pub(crate) fn new(entries: Vec<BoundOperation>, id: OperationGroupId) -> Self {
        let mut index: HashMap<NominalTy, HashMap<Arc<TypeIdentity>, ReceiverMethods>> =
            HashMap::new();
        for (position, operation) in entries.iter().enumerate() {
            let methods = index
                .entry(operation.requirement.interface.clone())
                .or_default()
                .entry(operation.receiver_identity.clone())
                .or_default();
            let slot = operation.slot as usize;
            methods
                .slots
                .resize(methods.slots.len().max(slot + 1), None);
            methods.slots[slot].get_or_insert(position);
            methods
                .members
                .entry(operation.requirement.member)
                .or_insert(position);
        }
        Self { id, entries, index }
    }

    pub(crate) fn operation_ids(&self) -> impl Iterator<Item = OperationId> + '_ {
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

    pub(crate) fn associated_interfaces(&self) -> impl Iterator<Item = AssociatedInterface> + '_ {
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
            .methods(receiver, interface)?
            .slots
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
                && operation.requirement.member == required.member
                && operation
                    .requirement
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
