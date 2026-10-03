//! Retained dynamic method and representation-adapter metadata.
use crate::{
    frame::types::{
        TypeEnvironment, arguments::ScopedSignature, compatibility::TypeView,
        operations::ReceiverOperations,
    },
    module::LoadedModule,
    value::Value,
};
use kagari_abi::{
    native_import::NativeSignature,
    types::{AbiType, GenericParameterAbi, NominalAbiType},
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::DefinitionId;
use std::{cell::OnceCell, rc::Rc};

#[derive(Debug, Clone)]
pub(crate) struct InterfaceMethodBinding {
    pub(crate) application: OnceCell<Rc<MethodApplication>>,
    pub(crate) receiver_operations: OnceCell<Option<Rc<ReceiverOperations>>>,
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
    pub(crate) prepared: OnceCell<Rc<InterfaceValueSnapshot>>,
    pub(crate) interface: NominalAbiType,
    pub(crate) binding: InterfaceResultBinding,
    pub(crate) view: bool,
}

#[derive(Debug)]
pub(crate) struct InterfaceValueSnapshot {
    pub(crate) receiver_operations: OnceCell<Rc<ReceiverOperations>>,
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) parents: Vec<InterfaceParentBinding>,
    pub(crate) data: Value,
    pub(crate) concrete_type: AbiType,
    pub(crate) concrete_expression: AbiType,
    pub(crate) interface_type: NominalAbiType,
    pub(crate) interface_expression: NominalAbiType,
    pub(crate) environment: Option<Rc<TypeEnvironment>>,
    pub(crate) implementation: LoadedModule,
    pub(crate) methods: Vec<Option<InterfaceMethodBinding>>,
}

#[derive(Debug)]
pub(crate) struct MethodApplication {
    pub(crate) signature: NativeSignature,
    pub(crate) scoped_signature: Option<ScopedSignature>,
    pub(crate) environment: Option<Rc<TypeEnvironment>>,
    pub(crate) result_adapter: Option<InterfaceResultBinding>,
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
