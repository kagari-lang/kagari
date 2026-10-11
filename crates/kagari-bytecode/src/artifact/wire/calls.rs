//! File-local bytecode call targets and aggregate field references.
use crate::{
    instruction::{CallTarget, FieldRef, NativeImportId, Register, RuntimeHelper, StructId},
    module::CallableTarget,
    program::ModuleRef,
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
};
use kagari_contract::{
    callable::wire::{InterfaceCallContractRecord, SharedCallRecord},
    ids::FunctionRef,
    standard::RuntimePrimitive,
};
use kagari_types::ty::wire::WireType;
use serde::{Deserialize, Serialize};

/// File-local FieldRef facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct FieldRefRecord {
    structure: StructId,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    arguments: Vec<WireType>,
    slot: u32,
}

impl FieldRefRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &FieldRef,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            structure: value.structure,
            arguments: value
                .arguments
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .collect::<Result<_, _>>()?,
            slot: value.slot,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<FieldRef, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(FieldRef {
            structure: self.structure,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
            slot: self.slot,
        })
    }
}

/// File-local CallTarget facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) enum CallTargetRecord {
    Shared {
        module: ModuleRef,
        target: CallableTarget,
        contract: Box<SharedCallRecord>,
    },
    ModuleFunction {
        module: ModuleRef,
        function: FunctionRef,
    },
    Function(FunctionRef),
    InterfaceMethod {
        module: ModuleRef,
        contract: Box<InterfaceCallContractRecord>,
    },
    Native(NativeImportId),
    Register(Register),
    ClosureRegister {
        register: Register,
        #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
        params: Vec<ValueType>,
        return_type: ValueType,
    },
    RuntimePrimitive(RuntimePrimitive),
    RuntimeHelper(RuntimeHelper),
}

impl CallTargetRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &CallTarget,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match value {
            CallTarget::Shared {
                module,
                target,
                contract,
            } => Self::Shared {
                module: *module,
                target: *target,
                contract: Box::new(SharedCallRecord::encode(
                    contract.as_ref(),
                    encoding,
                    cancel,
                )?),
            },
            CallTarget::ModuleFunction { module, function } => Self::ModuleFunction {
                module: *module,
                function: *function,
            },
            CallTarget::Function(field0) => Self::Function(*field0),
            CallTarget::InterfaceMethod { module, contract } => Self::InterfaceMethod {
                module: *module,
                contract: Box::new(InterfaceCallContractRecord::encode(
                    contract.as_ref(),
                    encoding,
                    cancel,
                )?),
            },
            CallTarget::Native(field0) => Self::Native(*field0),
            CallTarget::Register(field0) => Self::Register(*field0),
            CallTarget::ClosureRegister {
                register,
                params,
                return_type,
            } => Self::ClosureRegister {
                register: *register,
                params: params.clone(),
                return_type: *return_type,
            },
            CallTarget::RuntimePrimitive(field0) => Self::RuntimePrimitive(*field0),
            CallTarget::RuntimeHelper(field0) => Self::RuntimeHelper(field0.clone()),
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<CallTarget, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self {
            Self::Shared {
                module,
                target,
                contract,
            } => CallTarget::Shared {
                module,
                target,
                contract: Box::new((*contract).decode(decoding, cancel)?),
            },
            Self::ModuleFunction { module, function } => {
                CallTarget::ModuleFunction { module, function }
            }
            Self::Function(field0) => CallTarget::Function(field0),
            Self::InterfaceMethod { module, contract } => CallTarget::InterfaceMethod {
                module,
                contract: Box::new((*contract).decode(decoding, cancel)?),
            },
            Self::Native(field0) => CallTarget::Native(field0),
            Self::Register(field0) => CallTarget::Register(field0),
            Self::ClosureRegister {
                register,
                params,
                return_type,
            } => CallTarget::ClosureRegister {
                register,
                params,
                return_type,
            },
            Self::RuntimePrimitive(field0) => CallTarget::RuntimePrimitive(field0),
            Self::RuntimeHelper(field0) => CallTarget::RuntimeHelper(field0),
        })
    }
}
