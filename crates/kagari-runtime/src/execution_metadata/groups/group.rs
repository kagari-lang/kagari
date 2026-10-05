//! Immutable group membership and indexes; selected witnesses keep their own visibility.
use crate::{
    execution_metadata::{
        MetadataEdge,
        groups::{OperationGroupId, OperationId},
        operation::BoundOperation,
    },
    frame::types::bindings::AssociatedInterface,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{NominalTy, Ty},
};
use std::collections::HashMap;

#[derive(Debug, Default)]
struct ReceiverMethods {
    slots: Vec<Option<usize>>,
    members: HashMap<DefinitionId, usize>,
}

#[derive(Debug)]
pub(crate) struct OperationGroup {
    id: OperationGroupId,
    entries: Vec<BoundOperation>,
    index: HashMap<NominalTy<DefinitionId>, HashMap<Ty<DefinitionId>, ReceiverMethods>>,
}

impl OperationGroup {
    pub(crate) fn trace_metadata<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.extend(self.entries.iter().enumerate().map(|(member, _)| {
            MetadataEdge::Operation(OperationId {
                group: self.id,
                member,
            })
        }));
    }

    pub(crate) fn new(entries: Vec<BoundOperation>, id: OperationGroupId) -> Self {
        let mut index: HashMap<
            NominalTy<DefinitionId>,
            HashMap<Ty<DefinitionId>, ReceiverMethods>,
        > = HashMap::new();
        for (position, operation) in entries.iter().enumerate() {
            let methods = index
                .entry(operation.requirement.interface.clone())
                .or_default()
                .entry(operation.requirement.receiver.clone())
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

    pub(crate) fn get(&self, id: OperationId) -> Option<&BoundOperation> {
        (id.group == self.id)
            .then(|| self.entries.get(id.member))
            .flatten()
    }

    pub(crate) fn associated_interfaces(&self) -> impl Iterator<Item = AssociatedInterface> + '_ {
        self.entries.iter().map(|operation| AssociatedInterface {
            receiver: operation.requirement.receiver.clone(),
            interface: operation.associated_interface.clone(),
            owner: operation.owner.clone(),
        })
    }

    fn methods(
        &self,
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
    ) -> Option<&ReceiverMethods> {
        self.index.get(interface)?.get(receiver)
    }

    pub(crate) fn slot(
        &self,
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
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

    pub(crate) fn operation(
        &self,
        required: &NativeCallableRequirement<DefinitionId>,
    ) -> Option<OperationId> {
        let position = self
            .methods(&required.receiver, &required.interface)?
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
