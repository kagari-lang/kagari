//! File-local shared/interface calls and their generic operation witnesses.
use crate::{
    callable::{
        generic::GenericBody,
        interface::{InterfaceCallContract, TypeNormalization},
        receiver::{InterfaceInstance, ReceiverComparison},
        shared::SharedCall,
        witness::{OperationWitness, SharedMethodWitness},
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
use kagari_types::{
    declaration::wire::{
        leaf::{CallableImplementationRecord, NominalTyRecord},
        native::{NativeCallableRequirementRecord, SignatureRecord},
        signature::{GenericBoundRecord, GenericParamRecord},
    },
    ty::wire::WireType,
};
use serde::{Deserialize, Serialize};

/// File-local GenericBody facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(crate) struct GenericBodyRecord {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    parameters: Vec<GenericParamRecord>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    bounds: Vec<GenericBoundRecord>,
}

impl GenericBodyRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn encode(
        value: &GenericBody,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            parameters: value
                .parameters
                .iter()
                .map(|value| GenericParamRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            bounds: value
                .bounds
                .iter()
                .map(|value| GenericBoundRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<GenericBody, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(GenericBody {
            parameters: self
                .parameters
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            bounds: self
                .bounds
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local SharedMethodWitness facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(crate) struct SharedMethodWitnessRecord {
    requirement: NativeCallableRequirementRecord,
    implementation: ConcreteFunctionIdentityRecord,
}

impl SharedMethodWitnessRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn encode(
        value: &SharedMethodWitness,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            requirement: NativeCallableRequirementRecord::encode(
                &value.requirement,
                encoding,
                cancel,
            )?,
            implementation: ConcreteFunctionIdentityRecord::encode(
                &value.implementation,
                encoding,
                cancel,
            )?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<SharedMethodWitness, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(SharedMethodWitness {
            requirement: self.requirement.decode(decoding, cancel)?,
            implementation: self.implementation.decode(decoding, cancel)?,
        })
    }
}

pub(crate) mod operations;
use operations::OperationWitnessRecord;

/// File-local SharedCall facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct SharedCallRecord {
    entry: SharedCallEntryRecord,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    operations: Vec<OperationWitnessRecord>,
}

#[derive(Serialize, Deserialize)]
struct SharedCallEntryRecord {
    instance: ConcreteFunctionIdentityRecord,
    implementation: CallableImplementationRecord,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    arguments: Vec<WireType>,
    signature: SignatureRecord,
}

impl SharedCallRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &SharedCall,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            entry: SharedCallEntryRecord::encode(value, encoding, cancel)?,
            operations: value
                .operations
                .iter()
                .map(|value| OperationWitnessRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<SharedCall, DefinitionMappingError> {
        check_cancel(cancel)?;
        let operations = self
            .operations
            .into_iter()
            .map(|value| value.decode(decoding, cancel))
            .collect::<Result<_, _>>()?;
        self.entry.decode(operations, decoding, cancel)
    }
}

impl SharedCallEntryRecord {
    fn encode(
        value: &SharedCall,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            instance: ConcreteFunctionIdentityRecord::encode(&value.instance, encoding, cancel)?,
            implementation: CallableImplementationRecord::encode(
                &value.implementation,
                encoding,
                cancel,
            )?,
            arguments: value
                .arguments
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .collect::<Result<_, _>>()?,
            signature: SignatureRecord::encode(&value.signature, encoding, cancel)?,
        })
    }

    fn decode(
        self,
        operations: Vec<OperationWitness>,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<SharedCall, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(SharedCall {
            instance: self.instance.decode(decoding, cancel)?,
            implementation: self.implementation.decode(decoding, cancel)?,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
            signature: self.signature.decode(decoding, cancel)?,
            operations,
        })
    }
}

/// File-local InterfaceCallContract facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct InterfaceCallContractRecord {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    normalizations: Vec<TypeNormalizationRecord>,
    receiver: Option<WireType>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    operations: Vec<OperationWitnessRecord>,
    interface: NominalTyRecord,
    method_slot: u32,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    arguments: Vec<WireType>,
}

impl InterfaceCallContractRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &InterfaceCallContract,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            normalizations: value
                .normalizations
                .iter()
                .map(|value| TypeNormalizationRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            receiver: value
                .receiver
                .as_ref()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .transpose()?,
            operations: value
                .operations
                .iter()
                .map(|value| OperationWitnessRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            interface: NominalTyRecord::encode(&value.interface, encoding, cancel)?,
            method_slot: value.method_slot,
            arguments: value
                .arguments
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<InterfaceCallContract, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(InterfaceCallContract {
            normalizations: self
                .normalizations
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            receiver: self
                .receiver
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .transpose()?,
            operations: self
                .operations
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            interface: self.interface.decode(decoding, cancel)?,
            method_slot: self.method_slot,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local TypeNormalization facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
struct TypeNormalizationRecord {
    source: WireType,
    result: WireType,
}

impl TypeNormalizationRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn encode(
        value: &TypeNormalization,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            source: WireType::encode(&value.source, encoding, cancel)?,
            result: WireType::encode(&value.result, encoding, cancel)?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<TypeNormalization, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(TypeNormalization {
            source: self.source.decode(decoding, cancel)?,
            result: self.result.decode(decoding, cancel)?,
        })
    }
}

/// Portable receiver selection. Loading must recheck the carried operation.
#[derive(Serialize, Deserialize)]
pub struct ReceiverComparisonRecord {
    operation: OperationWitnessRecord,
}

impl ReceiverComparisonRecord {
    pub fn encode(
        value: &ReceiverComparison,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            operation: OperationWitnessRecord::encode(&value.operation, encoding, cancel)?,
        })
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ReceiverComparison, DefinitionMappingError> {
        Ok(ReceiverComparison {
            operation: self.operation.decode(decoding, cancel)?,
        })
    }
}

/// A portable interface demand and its separately checked receiver comparison.
#[derive(Serialize, Deserialize)]
pub struct InterfaceInstanceRecord {
    instance: ConcreteFunctionIdentityRecord,
    comparison: Option<ReceiverComparisonRecord>,
}

impl InterfaceInstanceRecord {
    pub fn encode(
        value: &InterfaceInstance,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            instance: ConcreteFunctionIdentityRecord::encode(&value.identity(), encoding, cancel)?,
            comparison: value
                .comparison
                .as_ref()
                .map(|value| ReceiverComparisonRecord::encode(value, encoding, cancel))
                .transpose()?,
        })
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<InterfaceInstance, DefinitionMappingError> {
        let instance = self.instance.decode(decoding, cancel)?;
        Ok(InterfaceInstance {
            declaration: instance.declaration,
            arguments: instance.arguments,
            comparison: self
                .comparison
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
        })
    }
}
