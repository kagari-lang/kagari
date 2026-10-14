//! File-local Native imports, selected callables and result adapters.
use crate::{
    callable::wire::{GenericBodyRecord, operations::OperationWitnessRecord},
    effects::EffectSet,
    native_import::{
        NativeImport,
        callables::{NativeCallableApplication, NativeCallableOrigin},
        result::NativeResultAdapter,
    },
    types::wire::module::ConcreteFunctionIdentityRecord,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use kagari_types::{
    declaration::wire::{
        leaf::CallableImplementationRecord,
        native::{NativeCallableRequirementRecord, SignatureRecord},
        signature::GenericBoundRecord,
    },
    host_interface::wire::declarations::WireHostFunctionDeclaration,
    reference::ReferenceAccess,
    ty::wire::WireType,
};
use serde::{Deserialize, Serialize};

/// File-local NativeImport facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct NativeImportRecord {
    receiver_access: Option<ReferenceAccess>,
    result_adapter: Option<NativeResultAdapterRecord>,
    generic: Option<GenericBodyRecord>,
    instance: ConcreteFunctionIdentityRecord,
    binding: PortableDefinitionRef,
    signature: SignatureRecord,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    requirements: Vec<GenericBoundRecord>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    callables: Vec<OperationWitnessRecord>,
    host: Option<WireHostFunctionDeclaration>,
}

impl NativeImportRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &NativeImport,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            receiver_access: value.receiver_access,
            result_adapter: value
                .result_adapter
                .as_ref()
                .map(|value| NativeResultAdapterRecord::encode(value, encoding, cancel))
                .transpose()?,
            generic: value
                .generic
                .as_ref()
                .map(|value| GenericBodyRecord::encode(value, encoding, cancel))
                .transpose()?,
            instance: ConcreteFunctionIdentityRecord::encode(&value.instance, encoding, cancel)?,
            binding: encoding.reference(value.binding)?,
            signature: SignatureRecord::encode(&value.signature, encoding, cancel)?,
            requirements: value
                .requirements
                .iter()
                .map(|value| GenericBoundRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            callables: value
                .callables
                .iter()
                .map(|value| OperationWitnessRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            host: value
                .host
                .as_ref()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireHostFunctionDeclaration::encode(
                        value, encoding, cancel,
                    )?)
                })
                .transpose()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<NativeImport, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(NativeImport {
            receiver_access: self.receiver_access,
            result_adapter: self
                .result_adapter
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
            generic: self
                .generic
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
            instance: self.instance.decode(decoding, cancel)?,
            binding: decoding.resolve(self.binding)?,
            signature: self.signature.decode(decoding, cancel)?,
            requirements: self
                .requirements
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            callables: self
                .callables
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            host: self
                .host
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .transpose()?,
        })
    }
}

/// File-local NativeCallableApplication facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct NativeCallableApplicationRecord {
    origin: NativeCallableOrigin,
    requirement: NativeCallableRequirementRecord,
    instance: ConcreteFunctionIdentityRecord,
    implementation: CallableImplementationRecord,
    signature: SignatureRecord,
    effects: EffectSet,
}

impl NativeCallableApplicationRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &NativeCallableApplication,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            origin: value.origin.clone(),
            requirement: NativeCallableRequirementRecord::encode(
                &value.requirement,
                encoding,
                cancel,
            )?,
            instance: ConcreteFunctionIdentityRecord::encode(&value.instance, encoding, cancel)?,
            implementation: CallableImplementationRecord::encode(
                &value.implementation,
                encoding,
                cancel,
            )?,
            signature: SignatureRecord::encode(&value.signature, encoding, cancel)?,
            effects: value.effects,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<NativeCallableApplication, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(NativeCallableApplication {
            origin: self.origin,
            requirement: self.requirement.decode(decoding, cancel)?,
            instance: self.instance.decode(decoding, cancel)?,
            implementation: self.implementation.decode(decoding, cancel)?,
            signature: self.signature.decode(decoding, cancel)?,
            effects: self.effects,
        })
    }
}

/// File-local NativeResultAdapter facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct NativeResultAdapterRecord {
    receiver: WireType,
    implementation: ConcreteFunctionIdentityRecord,
}

impl NativeResultAdapterRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &NativeResultAdapter,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            receiver: WireType::encode(&value.receiver, encoding, cancel)?,
            implementation: ConcreteFunctionIdentityRecord::encode(
                &value.implementation,
                encoding,
                cancel,
            )?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<NativeResultAdapter, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(NativeResultAdapter {
            receiver: self.receiver.decode(decoding, cancel)?,
            implementation: self.implementation.decode(decoding, cancel)?,
        })
    }
}
