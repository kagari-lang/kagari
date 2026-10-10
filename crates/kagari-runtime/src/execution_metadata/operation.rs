//! Checked executable selections and their generic invocation metadata.
use crate::{
    frame::types::TypeEnvironment, gc::interfaces::InterfaceResultBinding, module::LoadedModule,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{
    callable::Signature,
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericParam, NominalTy, Ty},
};

#[derive(Debug, Clone)]
pub(crate) struct BoundOperation {
    pub(crate) generic: Option<BoundGenericMethod>,
    pub(crate) requirement: NativeCallableRequirement<DefinitionId>,
    pub(crate) associated_interface: NominalTy<DefinitionId>,
    pub(crate) slot: u32,
    pub(crate) primitive: Option<RuntimePrimitive>,
    pub(crate) owner: LoadedModule,
    pub(crate) target: CallableTarget,
    pub(crate) signature: Signature<DefinitionId>,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundGenericMethod {
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) receiver_environment: Option<TypeEnvironment>,
    pub(crate) parameters: Vec<GenericParam<DefinitionId>>,
    pub(crate) entry_parameters: Vec<GenericParam<DefinitionId>>,
    pub(crate) entry_arguments: Vec<Ty<DefinitionId>>,
}

impl BoundOperation {
    pub(crate) fn matches_requirement(
        &self,
        required: &NativeCallableRequirement<DefinitionId>,
    ) -> bool {
        self.requirement == *required
            || (self.generic.is_some()
                && self.requirement.receiver == required.receiver
                && self.requirement.interface == required.interface
                && self.requirement.member == required.member)
    }
}
