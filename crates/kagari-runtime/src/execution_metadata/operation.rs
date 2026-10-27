//! Checked executable selections and their generic invocation metadata.
use crate::{
    execution_metadata::{MetadataEdge, applications::ApplicationId},
    frame::types::{TypeEnvironment, compatibility::TypeIdentity},
    gc::interfaces::InterfaceResultBinding,
    module::ExecutableModule,
};
use kagari_bytecode::module::CallableTarget;
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{
    callable::Signature,
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericParam, NominalTy, Ty},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct BoundOperation {
    pub(crate) receiver_identity: Arc<TypeIdentity>,
    pub(crate) application: Option<ApplicationId>,
    pub(crate) generic: Option<BoundGenericMethod>,
    pub(crate) requirement: NativeCallableRequirement,
    pub(crate) associated_interface: NominalTy,
    pub(crate) slot: u32,
    pub(crate) primitive: Option<RuntimePrimitive>,
    pub(crate) owner: ExecutableModule,
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
    pub(crate) fn scan_field(&self, field: usize) -> Option<MetadataEdge<'_>> {
        match field {
            0 => Some(MetadataEdge::Program(&self.owner)),
            1 => self.application.map(MetadataEdge::Application),
            2 | 3 => self.generic.as_ref()?.receiver_table.scan_field(field - 2),
            4 => self
                .generic
                .as_ref()?
                .receiver_environment
                .as_ref()
                .map(|environment| MetadataEdge::Environment(environment.id)),
            _ => None,
        }
    }

    pub(crate) fn matches_requirement(&self, required: &NativeCallableRequirement) -> bool {
        self.requirement.receiver == required.receiver
            && self.requirement.member == required.member
            && self.associated_interface.satisfies(&required.interface)
            && (self.generic.is_some() || self.requirement.arguments == required.arguments)
    }
}
