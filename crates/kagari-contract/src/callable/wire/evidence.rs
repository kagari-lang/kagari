//! A flat heterogeneous tree bounds method/construction dependencies before decoding.
use crate::{
    callable::{
        adapter::SharedAdapterWitness,
        construction::{
            evidence::{ConstructionSelection, ConstructionWitness},
            wire::ConstructionRequirementRecord,
        },
        evidence::EvidenceRef,
        wire::{SharedCallEntryRecord, SharedMethodWitnessRecord},
        witness::OperationWitness,
    },
    native_import::wire::NativeCallableApplicationRecord,
    types::wire::module::ConcreteFunctionIdentityRecord,
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

/// File-local facts only. The requested root kind and all linked proofs are checked
/// independently; a construction node cannot occupy a method-operation position.
#[derive(Serialize, Deserialize)]
pub struct EvidenceRecord {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    nodes: Vec<Node>,
}

#[derive(Serialize, Deserialize)]
enum Node {
    Selected(Box<NativeCallableApplicationRecord>),
    SharedMethod {
        entry: Box<SharedMethodWitnessRecord>,
        operations: u32,
        constructions: u32,
    },
    Forward(Box<NativeCallableRequirementRecord>),
    SharedAdapter {
        requirement: NativeCallableRequirementRecord,
        entry: Box<SharedCallEntryRecord>,
        operations: u32,
        constructions: u32,
    },
    Construction {
        requirement: ConstructionRequirementRecord,
        selection: Selection,
    },
}

#[derive(Serialize, Deserialize)]
enum Selection {
    Declared {
        implementation: ConcreteFunctionIdentityRecord,
        operations: u32,
        constructions: u32,
    },
    Interface,
    Forward,
}

enum Evidence {
    Operation(OperationWitness),
    Construction(ConstructionWitness),
}

impl EvidenceRecord {
    pub fn encode_operation(
        value: &OperationWitness,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        Self::encode(EvidenceRef::Operation(value), encoding, cancel)
    }

    pub fn encode_construction(
        value: &ConstructionWitness,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        Self::encode(EvidenceRef::Construction(value), encoding, cancel)
    }

    fn encode(
        value: EvidenceRef<'_>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        value.check_shape(cancel)?;
        let mut pending = vec![value];
        let mut nodes = Vec::new();
        while let Some(value) = pending.pop() {
            check_cancel(cancel)?;
            let (operations, constructions) = value.children();
            pending.extend(constructions.iter().rev().map(EvidenceRef::Construction));
            pending.extend(operations.iter().rev().map(EvidenceRef::Operation));
            nodes.push(match value {
                EvidenceRef::Operation(OperationWitness::Selected(value)) => {
                    Node::Selected(Box::new(NativeCallableApplicationRecord::encode(
                        value, encoding, cancel,
                    )?))
                }
                EvidenceRef::Operation(OperationWitness::Forward(value)) => {
                    Node::Forward(Box::new(NativeCallableRequirementRecord::encode(
                        value, encoding, cancel,
                    )?))
                }
                EvidenceRef::Operation(OperationWitness::SharedMethod(value)) => {
                    Node::SharedMethod {
                        entry: Box::new(SharedMethodWitnessRecord::encode(
                            value, encoding, cancel,
                        )?),
                        operations: operations.len() as u32,
                        constructions: constructions.len() as u32,
                    }
                }
                EvidenceRef::Operation(OperationWitness::SharedAdapter(value)) => {
                    Node::SharedAdapter {
                        requirement: NativeCallableRequirementRecord::encode(
                            &value.requirement,
                            encoding,
                            cancel,
                        )?,
                        entry: Box::new(SharedCallEntryRecord::encode(
                            &value.call,
                            encoding,
                            cancel,
                        )?),
                        operations: operations.len() as u32,
                        constructions: constructions.len() as u32,
                    }
                }
                EvidenceRef::Construction(value) => Node::Construction {
                    requirement: ConstructionRequirementRecord::encode(
                        &value.requirement,
                        encoding,
                        cancel,
                    )?,
                    selection: match &value.selection {
                        ConstructionSelection::Declared { implementation, .. } => {
                            Selection::Declared {
                                implementation: ConcreteFunctionIdentityRecord::encode(
                                    implementation,
                                    encoding,
                                    cancel,
                                )?,
                                operations: operations.len() as u32,
                                constructions: constructions.len() as u32,
                            }
                        }
                        ConstructionSelection::Interface => Selection::Interface,
                        ConstructionSelection::Forward => Selection::Forward,
                    },
                },
            });
        }
        Ok(Self { nodes })
    }

    pub fn decode_operation(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<OperationWitness, DefinitionMappingError> {
        match self.decode(decoding, cancel)? {
            Evidence::Operation(value) => Ok(value),
            _ => Err(DefinitionMappingError::InvalidContract),
        }
    }

    pub fn decode_construction(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ConstructionWitness, DefinitionMappingError> {
        match self.decode(decoding, cancel)? {
            Evidence::Construction(value) => Ok(value),
            _ => Err(DefinitionMappingError::InvalidContract),
        }
    }

    fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<Evidence, DefinitionMappingError> {
        if self.nodes.len() > 4096 {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        let mut pending: Vec<(Evidence, usize)> = Vec::new();
        for node in self.nodes.into_iter().rev() {
            check_cancel(cancel)?;
            let (operation_count, construction_count) = match &node {
                Node::SharedMethod {
                    operations,
                    constructions,
                    ..
                }
                | Node::SharedAdapter {
                    operations,
                    constructions,
                    ..
                }
                | Node::Construction {
                    selection:
                        Selection::Declared {
                            operations,
                            constructions,
                            ..
                        },
                    ..
                } => (*operations as usize, *constructions as usize),
                _ => (0, 0),
            };
            if operation_count > pending.len()
                || construction_count > pending.len() - operation_count
            {
                return Err(DefinitionMappingError::InvalidContract);
            }
            let mut depth = 0;
            let mut operations = Vec::new();
            let mut constructions = Vec::new();
            for index in 0..operation_count + construction_count {
                let (child, child_depth) = pending
                    .pop()
                    .ok_or(DefinitionMappingError::InvalidContract)?;
                depth = depth.max(child_depth + 1);
                if depth > 64 {
                    return Err(DefinitionMappingError::LimitExceeded);
                }
                match child {
                    Evidence::Operation(value) if index < operation_count => operations.push(value),
                    Evidence::Construction(value) if index >= operation_count => {
                        constructions.push(value)
                    }
                    _ => return Err(DefinitionMappingError::InvalidContract),
                }
            }
            let value =
                match node {
                    Node::Selected(value) => Evidence::Operation(OperationWitness::Selected(
                        Box::new(value.decode(decoding, cancel)?),
                    )),
                    Node::Forward(value) => Evidence::Operation(OperationWitness::Forward(
                        Box::new(value.decode(decoding, cancel)?),
                    )),
                    Node::SharedMethod { entry, .. } => {
                        Evidence::Operation(OperationWitness::SharedMethod(Box::new(
                            entry.decode(operations, constructions, decoding, cancel)?,
                        )))
                    }
                    Node::SharedAdapter {
                        requirement, entry, ..
                    } => Evidence::Operation(OperationWitness::SharedAdapter(Box::new(
                        SharedAdapterWitness {
                            requirement: requirement.decode(decoding, cancel)?,
                            call: entry.decode(operations, constructions, decoding, cancel)?,
                        },
                    ))),
                    Node::Construction {
                        requirement,
                        selection,
                    } => Evidence::Construction(ConstructionWitness {
                        requirement: requirement.decode(decoding, cancel)?,
                        selection: match selection {
                            Selection::Declared { implementation, .. } => {
                                ConstructionSelection::Declared {
                                    implementation: implementation.decode(decoding, cancel)?,
                                    operations,
                                    constructions,
                                }
                            }
                            Selection::Interface => ConstructionSelection::Interface,
                            Selection::Forward => ConstructionSelection::Forward,
                        },
                    }),
                };
            pending.push((value, depth));
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

#[cfg(test)]
mod construction_tests;
