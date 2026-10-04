//! Retained dynamic method and representation-adapter metadata.
use crate::{
    frame::types::{
        TypeEnvironment, arguments::ScopedSignature, compatibility::TypeView,
        operations::ReceiverOperations,
    },
    module::LoadedModule,
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_types::{
    callable::Signature,
    ty::{GenericParam, NominalTy, Ty},
};
use std::{cell::OnceCell, rc::Rc};

#[derive(Debug, Clone)]
pub(crate) struct InterfaceMethodBinding {
    pub(crate) application: OnceCell<Rc<MethodApplication>>,
    pub(crate) receiver_operations: OnceCell<Option<Rc<ReceiverOperations>>>,
    pub(crate) parameters: Vec<GenericParam<DefinitionId>>,
    pub(crate) entry_parameters: Vec<GenericParam<DefinitionId>>,
    pub(crate) entry_arguments: Vec<Ty<DefinitionId>>,
    pub(crate) result_adapter: Option<InterfaceResultBinding>,
    pub(crate) method: DefinitionId,
    pub(crate) target: CallableTarget,
    pub(crate) parameter_types: Vec<Ty<DefinitionId>>,
    pub(crate) return_type: Ty<DefinitionId>,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceResultBinding {
    pub(crate) owner: LoadedModule,
    pub(crate) table: usize,
    pub(crate) arguments: Vec<Ty<DefinitionId>>,
    pub(crate) environment: Option<Rc<TypeEnvironment>>,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceParentBinding {
    pub(crate) prepared: OnceCell<Rc<InterfaceValueSnapshot>>,
    pub(crate) interface: NominalTy<DefinitionId>,
    pub(crate) binding: InterfaceResultBinding,
    pub(crate) view: bool,
}

#[derive(Debug)]
pub(crate) struct InterfaceValueSnapshot {
    pub(crate) receiver_operations: OnceCell<Rc<ReceiverOperations>>,
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) parents: Vec<InterfaceParentBinding>,
    pub(crate) data: Value,
    pub(crate) concrete_type: Ty<DefinitionId>,
    pub(crate) concrete_expression: Ty<DefinitionId>,
    pub(crate) interface_type: NominalTy<DefinitionId>,
    pub(crate) interface_expression: NominalTy<DefinitionId>,
    pub(crate) environment: Option<Rc<TypeEnvironment>>,
    pub(crate) implementation: LoadedModule,
    pub(crate) methods: Vec<Option<InterfaceMethodBinding>>,
}

#[derive(Debug)]
pub(crate) struct MethodApplication {
    pub(crate) signature: Signature<DefinitionId>,
    pub(crate) scoped_signature: Option<ScopedSignature>,
    pub(crate) environment: Option<Rc<TypeEnvironment>>,
    pub(crate) result_adapter: Option<InterfaceResultBinding>,
}

impl InterfaceValueSnapshot {
    pub(crate) fn matches_type(
        &self,
        ty: &Ty<DefinitionId>,
        owner: &LoadedModule,
        environment: Option<&TypeEnvironment>,
    ) -> bool {
        TypeView::new(
            &Ty::Trait(self.interface_expression.clone()),
            &self.implementation,
            self.environment.as_deref(),
        )
        .compatible(TypeView::new(ty, owner, environment))
    }
}
