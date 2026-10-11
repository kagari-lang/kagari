//! Checked executable selections and their generic invocation metadata.
use crate::{
    frame::types::TypeEnvironment, gc::interfaces::InterfaceResultBinding, module::LoadedModule,
};
use kagari_bytecode::module::CallableTarget;
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{
    callable::Signature,
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericParam, NominalTy, Ty},
};

#[derive(Debug, Clone)]
pub(crate) struct BoundOperation {
    pub(crate) generic: Option<BoundGenericMethod>,
    pub(crate) requirement: NativeCallableRequirement,
    pub(crate) associated_interface: NominalTy,
    pub(crate) slot: u32,
    pub(crate) primitive: Option<RuntimePrimitive>,
    pub(crate) owner: LoadedModule,
    pub(crate) target: CallableTarget,
    pub(crate) signature: Signature,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundGenericMethod {
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) receiver_environment: Option<TypeEnvironment>,
    pub(crate) parameters: Vec<GenericParam>,
    pub(crate) entry_parameters: Vec<GenericParam>,
    pub(crate) entry_arguments: Vec<Ty>,
}

impl BoundOperation {
    pub(crate) fn matches_requirement(&self, required: &NativeCallableRequirement) -> bool {
        self.requirement.receiver == required.receiver
            && self.requirement.member == required.member
            && self.associated_interface.satisfies(&required.interface)
            && (self.generic.is_some() || self.requirement.arguments == required.arguments)
    }
}
