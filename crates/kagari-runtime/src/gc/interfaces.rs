//! Retained dynamic method and representation-adapter metadata.
use crate::{
    frame::types::{TypeEnvironment, compatibility::TypeView},
    module::LoadedModule,
    value::Value,
};
use kagari_abi::types::{AbiType, GenericParameterAbi, NominalAbiType};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::DefinitionId;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub(crate) struct InterfaceMethodBinding {
    pub(crate) parameters: Vec<GenericParameterAbi>,
    pub(crate) entry_parameters: Vec<GenericParameterAbi>,
    pub(crate) entry_arguments: Vec<AbiType>,
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
    pub(crate) arguments: Vec<AbiType>,
    pub(crate) environment: Option<Rc<TypeEnvironment>>,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceParentBinding {
    pub(crate) interface: NominalAbiType,
    pub(crate) binding: InterfaceResultBinding,
    pub(crate) view: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceValueSnapshot {
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) parents: Vec<InterfaceParentBinding>,
    pub(crate) data: Value,
    pub(crate) concrete_type: AbiType,
    pub(crate) interface_type: NominalAbiType,
    pub(crate) interface_expression: NominalAbiType,
    pub(crate) environment: Option<Rc<TypeEnvironment>>,
    pub(crate) implementation: LoadedModule,
    pub(crate) methods: Vec<Option<InterfaceMethodBinding>>,
}

impl InterfaceValueSnapshot {
    pub(crate) fn matches_type(
        &self,
        ty: &AbiType,
        owner: &LoadedModule,
        environment: Option<&TypeEnvironment>,
    ) -> bool {
        TypeView::new(
            &AbiType::Trait(self.interface_expression.clone()),
            &self.implementation,
            self.environment.as_deref(),
        )
        .compatible(TypeView::new(ty, owner, environment))
    }
}
