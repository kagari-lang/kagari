//! Retained dynamic method and representation-adapter metadata.
use crate::{module::LoadedModule, value::Value};
use kagari_abi::types::{AbiType, NominalAbiType};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::DefinitionId;

#[derive(Debug, Clone)]
pub(crate) struct InterfaceMethodBinding {
    pub(crate) result_adapter: Option<InterfaceResultBinding>,
    pub(crate) method: DefinitionId,
    pub(crate) target: CallableTarget,
    pub(crate) parameter_types: Vec<AbiType>,
    pub(crate) return_type: AbiType,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceResultBinding {
    pub(crate) owner: LoadedModule,
    pub(crate) table: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceValueSnapshot {
    pub(crate) data: Value,
    pub(crate) concrete_type: AbiType,
    pub(crate) interface_type: NominalAbiType,
    pub(crate) implementation: LoadedModule,
    pub(crate) methods: Vec<Option<InterfaceMethodBinding>>,
}
