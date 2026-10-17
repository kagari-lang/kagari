//! File-local MIR call targets, aggregate fields and Host paths.
use crate::{
    ids::InstanceId,
    instruction::{
        AggregateFieldRef, CallTarget, MirValue, PathRef, RuntimeHelper, SourceFunctionContract,
    },
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use kagari_contract::{
    callable::wire::{InterfaceCallContractRecord, SharedCallRecord},
    native_import::wire::NativeImportRecord,
    standard::RuntimePrimitive,
};
use kagari_types::reference::ReferenceAccess;
use kagari_types::{
    declaration::wire::{leaf::NominalTyRecord, path::PathDeclarationRecord},
    ty::wire::WireType,
};
use serde::{Deserialize, Serialize};

/// File-local AggregateFieldRef facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(crate) struct AggregateFieldRefRecord {
    owner: NominalTyRecord,
    slot: usize,
    reference: Option<ReferenceAccess>,
}

impl AggregateFieldRefRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn encode(
        value: &AggregateFieldRef,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            owner: NominalTyRecord::encode(&value.owner, encoding, cancel)?,
            slot: value.slot,
            reference: value.reference,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<AggregateFieldRef, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(AggregateFieldRef {
            owner: self.owner.decode(decoding, cancel)?,
            slot: self.slot,
            reference: self.reference,
        })
    }
}

/// File-local PathRef facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(crate) struct PathRefRecord {
    declaration: Option<PathDeclarationRecord>,
    contract_fingerprint: u64,
    root_ty: ValueType,
    result_ty: ValueType,
    read_only: bool,
    debug_name: String,
}

impl PathRefRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn encode(
        value: &PathRef,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            declaration: value
                .declaration
                .as_ref()
                .map(|value| PathDeclarationRecord::encode(value, encoding, cancel))
                .transpose()?,
            contract_fingerprint: value.contract_fingerprint,
            root_ty: value.root_ty,
            result_ty: value.result_ty,
            read_only: value.read_only,
            debug_name: value.debug_name.clone(),
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<PathRef, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(PathRef {
            declaration: self
                .declaration
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
            contract_fingerprint: self.contract_fingerprint,
            root_ty: self.root_ty,
            result_ty: self.result_ty,
            read_only: self.read_only,
            debug_name: self.debug_name,
        })
    }
}

/// File-local CallTarget facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(crate) enum CallTargetRecord {
    Shared(Box<SharedCallRecord>),
    SourceFunction(Box<SourceFunctionContractRecord>),
    Function(InstanceId),
    InterfaceMethod(Box<InterfaceCallContractRecord>),
    Native(Box<NativeImportRecord>),
    Value(MirValue),
    Closure {
        value: MirValue,
        #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
        params: Vec<ValueType>,
        return_type: ValueType,
    },
    RuntimePrimitive(RuntimePrimitive),
    RuntimeHelper(RuntimeHelper),
}

impl CallTargetRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn encode(
        value: &CallTarget,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match value {
            CallTarget::Shared(field0) => Self::Shared(Box::new(SharedCallRecord::encode(
                field0.as_ref(),
                encoding,
                cancel,
            )?)),
            CallTarget::SourceFunction(field0) => Self::SourceFunction(Box::new(
                SourceFunctionContractRecord::encode(field0.as_ref(), encoding, cancel)?,
            )),
            CallTarget::Function(field0) => Self::Function(*field0),
            CallTarget::InterfaceMethod(field0) => Self::InterfaceMethod(Box::new(
                InterfaceCallContractRecord::encode(field0.as_ref(), encoding, cancel)?,
            )),
            CallTarget::Native(field0) => Self::Native(Box::new(NativeImportRecord::encode(
                field0.as_ref(),
                encoding,
                cancel,
            )?)),
            CallTarget::Value(field0) => Self::Value(*field0),
            CallTarget::Closure {
                value,
                params,
                return_type,
            } => Self::Closure {
                value: *value,
                params: params.clone(),
                return_type: *return_type,
            },
            CallTarget::RuntimePrimitive(field0) => Self::RuntimePrimitive(*field0),
            CallTarget::RuntimeHelper(field0) => Self::RuntimeHelper(field0.clone()),
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<CallTarget, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Shared(field0) => {
                CallTarget::Shared(Box::new((*field0).decode(decoding, cancel)?))
            }
            Self::SourceFunction(field0) => {
                CallTarget::SourceFunction(Box::new((*field0).decode(decoding, cancel)?))
            }
            Self::Function(field0) => CallTarget::Function(field0),
            Self::InterfaceMethod(field0) => {
                CallTarget::InterfaceMethod(Box::new((*field0).decode(decoding, cancel)?))
            }
            Self::Native(field0) => {
                CallTarget::Native(Box::new((*field0).decode(decoding, cancel)?))
            }
            Self::Value(field0) => CallTarget::Value(field0),
            Self::Closure {
                value,
                params,
                return_type,
            } => CallTarget::Closure {
                value,
                params,
                return_type,
            },
            Self::RuntimePrimitive(field0) => CallTarget::RuntimePrimitive(field0),
            Self::RuntimeHelper(field0) => CallTarget::RuntimeHelper(field0),
        })
    }
}

/// File-local SourceFunctionContract facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(crate) struct SourceFunctionContractRecord {
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    arguments: Vec<WireType>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    params: Vec<ValueType>,
    return_type: ValueType,
}

impl SourceFunctionContractRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn encode(
        value: &SourceFunctionContract,
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
            params: value.params.clone(),
            return_type: value.return_type,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<SourceFunctionContract, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(SourceFunctionContract {
            declaration: decoding.resolve(self.declaration)?,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
            params: self.params,
            return_type: self.return_type,
        })
    }
}
