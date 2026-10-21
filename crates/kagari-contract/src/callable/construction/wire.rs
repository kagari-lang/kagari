//! Flat construction records bound nesting before allocating recursive evidence.
use crate::{
    callable::{
        construction::evidence::{
            ConstructionRequirement, ConstructionSelection, ConstructionWitness,
        },
        wire::operations::OperationWitnessRecord,
    },
    types::wire::module::ConcreteFunctionIdentityRecord,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
};
use kagari_types::{declaration::wire::leaf::NominalTyRecord, ty::wire::WireType};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct ConstructionRequirementRecord {
    receiver: WireType,
    interface: NominalTyRecord,
}

impl ConstructionRequirementRecord {
    pub fn encode(
        value: &ConstructionRequirement,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            receiver: WireType::encode(&value.receiver, encoding, cancel)?,
            interface: NominalTyRecord::encode(&value.interface, encoding, cancel)?,
        })
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ConstructionRequirement, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(ConstructionRequirement {
            receiver: self.receiver.decode(decoding, cancel)?,
            interface: self.interface.decode(decoding, cancel)?,
        })
    }
}

/// File-local facts only; decoded selections still require linked validation.
#[derive(Serialize, Deserialize)]
pub struct ConstructionWitnessRecord {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    nodes: Vec<Node>,
}

#[derive(Serialize, Deserialize)]
struct Node {
    requirement: ConstructionRequirementRecord,
    selection: Selection,
}

#[derive(Serialize, Deserialize)]
enum Selection {
    Declared {
        implementation: ConcreteFunctionIdentityRecord,
        #[serde(deserialize_with = "crate::decode_limits::nested")]
        operations: Vec<OperationWitnessRecord>,
        children: u32,
    },
    Interface,
    Forward,
}

impl ConstructionWitnessRecord {
    pub fn encode(
        value: &ConstructionWitness,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        value.check_shape(cancel)?;
        let mut pending = vec![(value, 0)];
        let mut nodes = Vec::new();
        while let Some((witness, depth)) = pending.pop() {
            check_cancel(cancel)?;
            if depth > 64 || nodes.len() >= 4096 {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            let selection = match &witness.selection {
                ConstructionSelection::Declared {
                    implementation,
                    operations,
                    constructions,
                } => {
                    if constructions.len() > 4096 || operations.len() > 4096 {
                        return Err(DefinitionMappingError::LimitExceeded);
                    }
                    pending.extend(constructions.iter().rev().map(|child| (child, depth + 1)));
                    Selection::Declared {
                        implementation: ConcreteFunctionIdentityRecord::encode(
                            implementation,
                            encoding,
                            cancel,
                        )?,
                        operations: operations
                            .iter()
                            .map(|operation| {
                                OperationWitnessRecord::encode(operation, encoding, cancel)
                            })
                            .collect::<Result<_, _>>()?,
                        children: constructions.len() as u32,
                    }
                }
                ConstructionSelection::Interface => Selection::Interface,
                ConstructionSelection::Forward => Selection::Forward,
            };
            nodes.push(Node {
                requirement: ConstructionRequirementRecord::encode(
                    &witness.requirement,
                    encoding,
                    cancel,
                )?,
                selection,
            });
        }
        Ok(Self { nodes })
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ConstructionWitness, DefinitionMappingError> {
        if self.nodes.len() > 4096 {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        let mut pending: Vec<(ConstructionWitness, usize)> = Vec::new();
        for node in self.nodes.into_iter().rev() {
            check_cancel(cancel)?;
            let mut depth = 0;
            let selection = match node.selection {
                Selection::Declared {
                    implementation,
                    operations,
                    children,
                } => {
                    if children as usize > pending.len() {
                        return Err(DefinitionMappingError::InvalidContract);
                    }
                    let mut constructions = Vec::new();
                    for _ in 0..children {
                        let (child, child_depth) = pending
                            .pop()
                            .ok_or(DefinitionMappingError::InvalidContract)?;
                        depth = depth.max(child_depth + 1);
                        if depth > 64 {
                            return Err(DefinitionMappingError::LimitExceeded);
                        }
                        constructions.push(child);
                    }
                    ConstructionSelection::Declared {
                        implementation: implementation.decode(decoding, cancel)?,
                        operations: operations
                            .into_iter()
                            .map(|operation| operation.decode(decoding, cancel))
                            .collect::<Result<_, _>>()?,
                        constructions,
                    }
                }
                Selection::Interface => ConstructionSelection::Interface,
                Selection::Forward => ConstructionSelection::Forward,
            };
            pending.push((
                ConstructionWitness {
                    requirement: node.requirement.decode(decoding, cancel)?,
                    selection,
                },
                depth,
            ));
        }
        if pending.len() != 1 {
            return Err(DefinitionMappingError::InvalidContract);
        }
        let witness = pending
            .pop()
            .ok_or(DefinitionMappingError::InvalidContract)?
            .0;
        witness.check_shape(cancel)?;
        Ok(witness)
    }
}

#[cfg(test)]
mod tests;
