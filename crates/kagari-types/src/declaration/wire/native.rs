//! Native declaration, callable requirement and signature wire records.
use crate::{
    callable::Signature,
    declaration::{
        NativeDeclaration,
        requirement::NativeCallableRequirement,
        wire::{leaf::NominalTyRecord, signature::FnDeclRecord},
    },
    ty::wire::WireType,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::{
            DefinitionId,
            wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
        },
    },
};
use serde::{Deserialize, Serialize};

/// File-local NativeDeclaration facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct NativeDeclarationRecord {
    concrete_result: Option<WireType>,
    declaration: PortableDefinitionRef,
    function: FnDeclRecord,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    callable_requirements: Vec<NativeCallableRequirementRecord>,
}

impl NativeDeclarationRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &NativeDeclaration<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            concrete_result: value
                .concrete_result
                .as_ref()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .transpose()?,
            declaration: encoding.reference(value.declaration)?,
            function: FnDeclRecord::encode(&value.function, encoding, cancel)?,
            callable_requirements: value
                .callable_requirements
                .iter()
                .map(|value| NativeCallableRequirementRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<NativeDeclaration<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(NativeDeclaration {
            concrete_result: self
                .concrete_result
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .transpose()?,
            declaration: decoding.resolve(self.declaration)?,
            function: self.function.decode(decoding, cancel)?,
            callable_requirements: self
                .callable_requirements
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local NativeCallableRequirement facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct NativeCallableRequirementRecord {
    receiver: WireType,
    interface: NominalTyRecord,
    member: PortableDefinitionRef,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    arguments: Vec<WireType>,
}

impl NativeCallableRequirementRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &NativeCallableRequirement<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            receiver: WireType::encode(&value.receiver, encoding, cancel)?,
            interface: NominalTyRecord::encode(&value.interface, encoding, cancel)?,
            member: encoding.reference(value.member)?,
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
    ) -> Result<NativeCallableRequirement<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(NativeCallableRequirement {
            receiver: self.receiver.decode(decoding, cancel)?,
            interface: self.interface.decode(decoding, cancel)?,
            member: decoding.resolve(self.member)?,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local Signature facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct SignatureRecord {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    params: Vec<WireType>,
    result: WireType,
}

impl SignatureRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &Signature<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            params: value
                .params
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .collect::<Result<_, _>>()?,
            result: WireType::encode(&value.result, encoding, cancel)?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<Signature<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Signature {
            params: self
                .params
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
            result: self.result.decode(decoding, cancel)?,
        })
    }
}
