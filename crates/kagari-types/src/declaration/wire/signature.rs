//! Fixed wire signatures and binder records.
use crate::{
    callable::{MethodPolicy, NativeDefaultApplication},
    declaration::{
        FnDecl, Param,
        wire::leaf::{CallableImplementationRecord, ConstraintRecord},
    },
    decode_limits,
    ty::{GenericBound, GenericParam, wire::WireType},
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

/// Codec record for the scoped GenericParam declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
pub struct GenericParamRecord {
    owner: PortableDefinitionRef,
    position: usize,
}

impl GenericParamRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &GenericParam<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            owner: encoding.reference(value.owner)?,
            position: value.position,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<GenericParam<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(GenericParam {
            owner: decoding.resolve(self.owner)?,
            position: self.position,
        })
    }
}

/// Codec record for the scoped GenericBound declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
pub struct GenericBoundRecord {
    ty: WireType,
    #[serde(deserialize_with = "decode_limits::nested")]
    constraints: Vec<ConstraintRecord>,
}

impl GenericBoundRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &GenericBound<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            ty: WireType::encode(&value.ty, encoding, cancel)?,
            constraints: value
                .constraints
                .iter()
                .map(|value| ConstraintRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<GenericBound<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(GenericBound {
            ty: self.ty.decode(decoding, cancel)?,
            constraints: self
                .constraints
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// Codec record for the scoped NativeDefaultApplication declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
pub(super) struct NativeDefaultApplicationRecord {
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "decode_limits::nested")]
    arguments: Vec<WireType>,
}

impl NativeDefaultApplicationRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &NativeDefaultApplication<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            arguments: value
                .arguments
                .iter()
                .map(|value| WireType::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<NativeDefaultApplication<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(NativeDefaultApplication {
            declaration: decoding.resolve(self.declaration)?,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// Codec record for the scoped FnDecl declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
pub struct FnDeclRecord {
    method_policy: MethodPolicy,
    name: String,
    implementation: CallableImplementationRecord,
    #[serde(deserialize_with = "decode_limits::nested")]
    generic_params: Vec<GenericParamRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    bounds: Vec<GenericBoundRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    params: Vec<ParamRecord>,
    return_type: WireType,
}

impl FnDeclRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &FnDecl<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            method_policy: value.method_policy,
            name: value.name.clone(),
            implementation: CallableImplementationRecord::encode(
                &value.implementation,
                encoding,
                cancel,
            )?,
            generic_params: value
                .generic_params
                .iter()
                .map(|value| GenericParamRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            bounds: value
                .bounds
                .iter()
                .map(|value| GenericBoundRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            params: value
                .params
                .iter()
                .map(|value| ParamRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            return_type: WireType::encode(&value.return_type, encoding, cancel)?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<FnDecl<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(FnDecl {
            method_policy: self.method_policy,
            name: self.name,
            implementation: self.implementation.decode(decoding, cancel)?,
            generic_params: self
                .generic_params
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            bounds: self
                .bounds
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            params: self
                .params
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            return_type: self.return_type.decode(decoding, cancel)?,
        })
    }
}

/// Codec record for the scoped Param declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
pub(super) struct ParamRecord {
    name: String,
    ty: WireType,
    mutable: bool,
}

impl ParamRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &Param<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            name: value.name.clone(),
            ty: WireType::encode(&value.ty, encoding, cancel)?,
            mutable: value.mutable,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<Param<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Param {
            name: self.name,
            ty: self.ty.decode(decoding, cancel)?,
            mutable: self.mutable,
        })
    }
}
