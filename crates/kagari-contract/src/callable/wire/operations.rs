//! Flat operation trees bound deserialization before constructing recursive witnesses.
use crate::{
    callable::{
        adapter::SharedAdapterWitness,
        wire::{SharedCallEntryRecord, SharedMethodWitnessRecord},
        witness::OperationWitness,
    },
    native_import::wire::NativeCallableApplicationRecord,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
};
use kagari_types::declaration::wire::native::NativeCallableRequirementRecord;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct OperationWitnessRecord {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    nodes: Vec<Node>,
}

#[derive(Serialize, Deserialize)]
enum Node {
    Selected(Box<NativeCallableApplicationRecord>),
    SharedMethod {
        entry: Box<SharedMethodWitnessRecord>,
        children: u32,
    },
    Forward(Box<NativeCallableRequirementRecord>),
    SharedAdapter {
        requirement: NativeCallableRequirementRecord,
        entry: Box<SharedCallEntryRecord>,
        children: u32,
    },
}

impl OperationWitnessRecord {
    pub(crate) fn encode(
        value: &OperationWitness,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        let mut pending = vec![(value, 0)];
        let mut nodes = Vec::new();
        while let Some((operation, depth)) = pending.pop() {
            check_cancel(cancel)?;
            if depth > 64 || nodes.len() >= 4096 {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            if operation.children().len() > 4096 {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            pending.extend(
                operation
                    .children()
                    .iter()
                    .rev()
                    .map(|child| (child, depth + 1)),
            );
            nodes.push(match operation {
                OperationWitness::Selected(value) => Node::Selected(Box::new(
                    NativeCallableApplicationRecord::encode(value, encoding, cancel)?,
                )),
                OperationWitness::SharedMethod(value) => Node::SharedMethod {
                    entry: Box::new(SharedMethodWitnessRecord::encode(value, encoding, cancel)?),
                    children: value.operations.len() as u32,
                },
                OperationWitness::Forward(value) => Node::Forward(Box::new(
                    NativeCallableRequirementRecord::encode(value, encoding, cancel)?,
                )),
                OperationWitness::SharedAdapter(adapter) => Node::SharedAdapter {
                    requirement: NativeCallableRequirementRecord::encode(
                        &adapter.requirement,
                        encoding,
                        cancel,
                    )?,
                    entry: Box::new(SharedCallEntryRecord::encode(
                        &adapter.call,
                        encoding,
                        cancel,
                    )?),
                    children: adapter.call.operations.len() as u32,
                },
            });
        }
        Ok(Self { nodes })
    }

    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<OperationWitness, DefinitionMappingError> {
        if self.nodes.len() > 4096 {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        let mut pending: Vec<(OperationWitness, usize)> = Vec::new();
        for node in self.nodes.into_iter().rev() {
            check_cancel(cancel)?;
            let children = match &node {
                Node::SharedMethod { children, .. } | Node::SharedAdapter { children, .. } => {
                    *children
                }
                _ => 0,
            };
            if children as usize > pending.len() {
                return Err(DefinitionMappingError::InvalidContract);
            }
            let mut depth = 0;
            let mut operations = Vec::new();
            for _ in 0..children {
                let (operation, child_depth) = pending
                    .pop()
                    .ok_or(DefinitionMappingError::InvalidContract)?;
                depth = depth.max(child_depth + 1);
                if depth > 64 {
                    return Err(DefinitionMappingError::LimitExceeded);
                }
                operations.push(operation);
            }
            let operation = match node {
                Node::Selected(value) => {
                    OperationWitness::Selected(Box::new(value.decode(decoding, cancel)?))
                }
                Node::SharedMethod { entry, .. } => OperationWitness::SharedMethod(Box::new(
                    entry.decode(operations, decoding, cancel)?,
                )),
                Node::Forward(value) => {
                    OperationWitness::Forward(Box::new(value.decode(decoding, cancel)?))
                }
                Node::SharedAdapter {
                    requirement,
                    entry,
                    children: _,
                } => OperationWitness::SharedAdapter(Box::new(SharedAdapterWitness {
                    requirement: requirement.decode(decoding, cancel)?,
                    call: entry.decode(operations, decoding, cancel)?,
                })),
            };
            pending.push((operation, depth));
        }
        if pending.len() != 1 {
            return Err(DefinitionMappingError::InvalidContract);
        }
        Ok(pending
            .pop()
            .ok_or(DefinitionMappingError::InvalidContract)?
            .0)
    }
}

#[cfg(test)]
mod tests;
