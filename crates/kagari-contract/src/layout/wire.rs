//! File-local struct/enum layouts; runtime layout verification remains separate.
use crate::layout::{EnumLayout, EnumVariantLayout, StructFieldLayout, StructLayout};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use kagari_types::{ty::wire::WireType, visibility::Visibility};
use serde::{Deserialize, Serialize};

/// File-local EnumLayout facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct EnumLayoutRecord {
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    arguments: Vec<WireType>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    variants: Vec<EnumVariantLayoutRecord>,
}

impl EnumLayoutRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &EnumLayout,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            arguments: value
                .arguments
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .collect::<Result<_, _>>()?,
            variants: value
                .variants
                .iter()
                .map(|value| EnumVariantLayoutRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<EnumLayout, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(EnumLayout {
            declaration: decoding.resolve(self.declaration)?,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
            variants: self
                .variants
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local EnumVariantLayout facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
struct EnumVariantLayoutRecord {
    reports_failure: bool,
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    payload: Vec<WireType>,
}

impl EnumVariantLayoutRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn encode(
        value: &EnumVariantLayout,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            reports_failure: value.reports_failure,
            declaration: encoding.reference(value.declaration)?,
            payload: value
                .payload
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<EnumVariantLayout, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(EnumVariantLayout {
            reports_failure: self.reports_failure,
            declaration: decoding.resolve(self.declaration)?,
            payload: self
                .payload
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local StructLayout facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct StructLayoutRecord {
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    arguments: Vec<WireType>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    fields: Vec<StructFieldLayoutRecord>,
}

impl StructLayoutRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &StructLayout,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            arguments: value
                .arguments
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .collect::<Result<_, _>>()?,
            fields: value
                .fields
                .iter()
                .map(|value| StructFieldLayoutRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<StructLayout, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(StructLayout {
            declaration: decoding.resolve(self.declaration)?,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
            fields: self
                .fields
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local StructFieldLayout facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
struct StructFieldLayoutRecord {
    visibility: Visibility,
    declaration: PortableDefinitionRef,
    name: String,
    ty: WireType,
    mutable: bool,
}

impl StructFieldLayoutRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn encode(
        value: &StructFieldLayout,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            visibility: value.visibility,
            declaration: encoding.reference(value.declaration)?,
            name: value.name.clone(),
            ty: WireType::encode(&value.ty, encoding, cancel)?,
            mutable: value.mutable,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<StructFieldLayout, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(StructFieldLayout {
            visibility: self.visibility,
            declaration: decoding.resolve(self.declaration)?,
            name: self.name,
            ty: self.ty.decode(decoding, cancel)?,
            mutable: self.mutable,
        })
    }
}
