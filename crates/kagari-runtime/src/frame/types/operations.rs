//! Shared receiver selections and call-local witnesses retain their supplying group.
use crate::{frame::types::BoundOperation, module::LoadedModule};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{NominalTy, Ty},
};
use std::{collections::HashMap, rc::Rc};

#[derive(Debug, Default)]
struct ReceiverMethods {
    slots: Vec<Option<usize>>,
    members: HashMap<DefinitionId, usize>,
}

#[derive(Debug)]
pub(crate) struct ReceiverOperations {
    entries: Vec<Rc<BoundOperation>>,
    index: HashMap<NominalTy<DefinitionId>, HashMap<Ty<DefinitionId>, ReceiverMethods>>,
}

impl ReceiverOperations {
    pub(crate) fn new(entries: Vec<BoundOperation>) -> Rc<Self> {
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
        Rc::new_cyclic(|weak| Self {
            entries: entries
                .into_iter()
                .map(|mut operation| {
                    operation.receiver_operations = weak.clone();
                    Rc::new(operation)
                })
                .collect(),
            index,
        })
    }

    fn methods(
        &self,
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
    ) -> Option<&ReceiverMethods> {
        self.index.get(interface)?.get(receiver)
    }

    fn slot(
        &self,
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
        slot: u32,
    ) -> Option<&Rc<BoundOperation>> {
        let position = self
            .methods(receiver, interface)?
            .slots
            .get(slot as usize)?
            .as_ref()?;
        self.entries.get(*position)
    }

    pub(crate) fn operation(
        &self,
        required: &NativeCallableRequirement<DefinitionId>,
    ) -> Option<&Rc<BoundOperation>> {
        let position = self
            .methods(&required.receiver, &required.interface)?
            .members
            .get(&required.member)?;
        self.entries
            .get(*position)
            .filter(|operation| matches_requirement(operation, required))
    }
}

#[derive(Debug, Clone)]
enum OperationSegment {
    Selected(Rc<BoundOperation>),
    Receiver(Rc<ReceiverOperations>),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OperationBindings {
    segments: Vec<OperationSegment>,
    // A selected descriptor has only a Weak back-reference. Keep its immutable
    // group alive without exposing unrelated methods as caller-selected witnesses.
    retained: Vec<Rc<ReceiverOperations>>,
}

impl OperationBindings {
    pub(crate) fn associated_interfaces(
        &self,
    ) -> Vec<(Ty<DefinitionId>, NominalTy<DefinitionId>, LoadedModule)> {
        self.segments
            .iter()
            .flat_map(|segment| match segment {
                OperationSegment::Selected(operation) => vec![(
                    operation.requirement.receiver.clone(),
                    operation.associated_interface.clone(),
                    operation.owner.clone(),
                )],
                OperationSegment::Receiver(group) => group
                    .entries
                    .iter()
                    .map(|operation| {
                        (
                            operation.requirement.receiver.clone(),
                            operation.associated_interface.clone(),
                            operation.owner.clone(),
                        )
                    })
                    .collect(),
            })
            .collect()
    }
    pub(crate) fn associated_output(
        &self,
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
        member: DefinitionId,
    ) -> Option<(&Ty<DefinitionId>, &LoadedModule)> {
        fn output<'a>(
            operation: &'a BoundOperation,
            receiver: &Ty<DefinitionId>,
            interface: &NominalTy<DefinitionId>,
            member: DefinitionId,
        ) -> Option<(&'a Ty<DefinitionId>, &'a LoadedModule)> {
            let applied = &operation.associated_interface;
            (operation.requirement.receiver == *receiver
                && applied.declaration == interface.declaration
                && applied.arguments == interface.arguments
                && interface
                    .associated_types
                    .iter()
                    .all(|(id, ty)| applied.associated_types.get(id) == Some(ty)))
            .then(|| {
                applied
                    .associated_types
                    .get(&member)
                    .map(|ty| (ty, &operation.owner))
            })
            .flatten()
        }
        self.segments.iter().find_map(|segment| match segment {
            OperationSegment::Selected(operation) => output(operation, receiver, interface, member),
            OperationSegment::Receiver(group) => group
                .entries
                .iter()
                .find_map(|operation| output(operation, receiver, interface, member)),
        })
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub(crate) fn push(&mut self, operation: Rc<BoundOperation>) {
        if let Some(group) = operation.receiver_operations.upgrade()
            && !self.retained.iter().any(|held| Rc::ptr_eq(held, &group))
        {
            self.retained.push(group);
        }
        self.segments.push(OperationSegment::Selected(operation));
    }

    pub(crate) fn receiver(&mut self, group: Rc<ReceiverOperations>) {
        self.segments.push(OperationSegment::Receiver(group));
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.segments.extend(other.segments);
        self.retained.extend(other.retained);
    }

    pub(crate) fn operation_slot(
        &self,
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
        slot: u32,
    ) -> Option<&Rc<BoundOperation>> {
        self.segments.iter().find_map(|segment| match segment {
            OperationSegment::Selected(operation) => (operation.slot == slot
                && operation.requirement.receiver == *receiver
                && operation.requirement.interface == *interface)
                .then_some(operation),
            OperationSegment::Receiver(group) => group.slot(receiver, interface, slot),
        })
    }

    pub(crate) fn operation(
        &self,
        required: &NativeCallableRequirement<DefinitionId>,
    ) -> Option<&Rc<BoundOperation>> {
        self.segments.iter().find_map(|segment| match segment {
            OperationSegment::Selected(operation) => {
                matches_requirement(operation, required).then_some(operation)
            }
            OperationSegment::Receiver(group) => group.operation(required),
        })
    }
}

fn matches_requirement(
    operation: &BoundOperation,
    required: &NativeCallableRequirement<DefinitionId>,
) -> bool {
    operation.requirement == *required
        || (operation.generic.is_some()
            && operation.requirement.receiver == required.receiver
            && operation.requirement.interface == required.interface
            && operation.requirement.member == required.member)
}
