//! File-local interface tables, parent mappings and result adapters.
use crate::module::{
    CallableTarget, InterfaceMethodSlot, InterfaceParentRecord, InterfaceResultAdapter,
    InterfaceTableRecord, InterfaceViewRecord,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use kagari_contract::types::wire::module::ConcreteFunctionIdentityRecord;
use kagari_types::{declaration::wire::leaf::NominalTyRecord, ty::wire::WireType};
use serde::{Deserialize, Serialize};

/// File-local InterfaceTableRecord facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct InterfaceTableRecordWire {
    view: Option<InterfaceViewRecordWire>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    arguments: Vec<WireType>,
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    methods: Vec<InterfaceMethodSlotRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    parents: Vec<InterfaceParentRecordWire>,
}

impl InterfaceTableRecordWire {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &InterfaceTableRecord,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            view: value
                .view
                .as_ref()
                .map(|value| InterfaceViewRecordWire::encode(value, encoding, cancel))
                .transpose()?,
            arguments: value
                .arguments
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .collect::<Result<_, _>>()?,
            declaration: encoding.reference(value.declaration)?,
            methods: value
                .methods
                .iter()
                .map(|value| InterfaceMethodSlotRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            parents: value
                .parents
                .iter()
                .map(|value| InterfaceParentRecordWire::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<InterfaceTableRecord, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(InterfaceTableRecord {
            view: self
                .view
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
            declaration: decoding.resolve(self.declaration)?,
            methods: self
                .methods
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            parents: self
                .parents
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local InterfaceParentRecord facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct InterfaceParentRecordWire {
    interface: NominalTyRecord,
    implementation: ConcreteFunctionIdentityRecord,
    view: bool,
}

impl InterfaceParentRecordWire {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &InterfaceParentRecord,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            interface: NominalTyRecord::encode(&value.interface, encoding, cancel)?,
            implementation: ConcreteFunctionIdentityRecord::encode(
                &value.implementation,
                encoding,
                cancel,
            )?,
            view: value.view,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<InterfaceParentRecord, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(InterfaceParentRecord {
            interface: self.interface.decode(decoding, cancel)?,
            implementation: self.implementation.decode(decoding, cancel)?,
            view: self.view,
        })
    }
}

/// File-local InterfaceViewRecord facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct InterfaceViewRecordWire {
    interface: NominalTyRecord,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    results: Vec<InterfaceResultAdapterRecord>,
}

impl InterfaceViewRecordWire {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &InterfaceViewRecord,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            interface: NominalTyRecord::encode(&value.interface, encoding, cancel)?,
            results: value
                .results
                .iter()
                .map(|value| InterfaceResultAdapterRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<InterfaceViewRecord, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(InterfaceViewRecord {
            interface: self.interface.decode(decoding, cancel)?,
            results: self
                .results
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local InterfaceResultAdapter facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct InterfaceResultAdapterRecord {
    method: PortableDefinitionRef,
    implementation: ConcreteFunctionIdentityRecord,
}

impl InterfaceResultAdapterRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &InterfaceResultAdapter,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            method: encoding.reference(value.method)?,
            implementation: ConcreteFunctionIdentityRecord::encode(
                &value.implementation,
                encoding,
                cancel,
            )?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<InterfaceResultAdapter, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(InterfaceResultAdapter {
            method: decoding.resolve(self.method)?,
            implementation: self.implementation.decode(decoding, cancel)?,
        })
    }
}

/// File-local InterfaceMethodSlot facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct InterfaceMethodSlotRecord {
    method: PortableDefinitionRef,
    target: CallableTarget,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    arguments: Vec<WireType>,
}

impl InterfaceMethodSlotRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &InterfaceMethodSlot,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            method: encoding.reference(value.method)?,
            target: value.target,
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
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<InterfaceMethodSlot, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(InterfaceMethodSlot {
            method: decoding.resolve(self.method)?,
            target: self.target,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
        })
    }
}
