//! Reified type arguments retained by a shared frame or closure.
pub mod arguments;
pub(crate) mod compatibility;
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::interfaces::InterfaceResultBinding,
    module::{LoadedModule, RetainedRuntimeProgram},
};
use kagari_abi::{
    callable::generic::GenericBody,
    native_import::{NativeSignature, callables::NativeCallableRequirement},
    standard::RuntimePrimitive,
    types::{AbiType, GenericParameterAbi, NominalAbiType, substitution::TypeSubstitution},
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::DefinitionId;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub(crate) struct BoundOperation {
    pub(crate) generic: Option<BoundGenericMethod>,
    pub(crate) requirement: NativeCallableRequirement,
    pub(crate) slot: u32,
    pub(crate) primitive: Option<RuntimePrimitive>,
    pub(crate) owner: LoadedModule,
    pub(crate) target: CallableTarget,
    pub(crate) signature: NativeSignature,
    pub(crate) retention: Rc<RetainedRuntimeProgram>,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundGenericMethod {
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) receiver_environment: Option<Rc<TypeEnvironment>>,
    pub(crate) parameters: Vec<GenericParameterAbi>,
    pub(crate) entry_parameters: Vec<GenericParameterAbi>,
    pub(crate) entry_arguments: Vec<AbiType>,
}

#[derive(Debug, Clone)]
pub struct TypeEnvironment {
    parameters: Vec<GenericParameterAbi>,
    arguments: Vec<TypeArgument>,
    pub(crate) operations: Vec<Rc<BoundOperation>>,
}

impl TypeEnvironment {
    pub(crate) fn new(
        parameters: Vec<GenericParameterAbi>,
        arguments: Vec<TypeArgument>,
    ) -> Result<Self, RuntimeError> {
        if parameters.len() != arguments.len()
            || !arguments.iter().all(|argument| argument.ty().is_concrete())
        {
            return Err(RuntimeError::module_validation(
                "generic call type arguments",
            ));
        }
        Ok(Self {
            parameters,
            arguments,
            operations: vec![],
        })
    }

    /// Layout metadata cannot keep unrelated executable selections or module state alive.
    pub(crate) fn types_only(&self) -> Self {
        Self {
            parameters: self.parameters.clone(),
            arguments: self.arguments.clone(),
            operations: vec![],
        }
    }

    pub(crate) fn include(&mut self, parent: Option<&Self>) -> Result<(), RuntimeError> {
        if let Some(parent) = parent {
            if parent
                .parameters
                .iter()
                .any(|parameter| self.parameters.contains(parameter))
            {
                return Err(RuntimeError::module_validation(
                    "duplicate generic call binder",
                ));
            }
            self.parameters.extend(parent.parameters.iter().cloned());
            self.arguments.extend(parent.arguments.iter().cloned());
        }
        Ok(())
    }

    pub(crate) fn argument(&self, owner: &DefinitionId, position: usize) -> Option<&TypeArgument> {
        self.parameters
            .iter()
            .position(|parameter| parameter.owner == *owner && parameter.position == position)
            .and_then(|index| self.arguments.get(index))
    }

    pub(crate) fn matches(&self, body: &GenericBody) -> bool {
        self.parameters == body.parameters
    }

    pub(crate) fn resolve(&self, ty: &AbiType) -> Result<AbiType, RuntimeError> {
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in self.parameters.iter().zip(&self.arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument.ty());
        }
        let result = substitution
            .apply(ty, &Default::default())
            .map_err(|_| RuntimeError::module_validation("generic type substitution"))?;
        if !result.is_concrete() {
            return Err(RuntimeError::module_validation("unbound generic type"));
        }
        Ok(result)
    }

    pub(crate) fn resolve_requirement(
        &self,
        required: &NativeCallableRequirement,
    ) -> Result<NativeCallableRequirement, RuntimeError> {
        let AbiType::Trait(interface) =
            self.resolve(&AbiType::Trait(required.interface.clone()))?
        else {
            return Err(RuntimeError::module_validation("constraint interface type"));
        };
        Ok(NativeCallableRequirement {
            receiver: self.resolve(&required.receiver)?,
            interface,
            member: required.member.clone(),
            arguments: required
                .arguments
                .iter()
                .map(|ty| self.resolve(ty))
                .collect::<Result<_, _>>()?,
        })
    }

    pub(crate) fn operation_slot(
        &self,
        receiver: &AbiType,
        interface: &NominalAbiType,
        slot: u32,
    ) -> Option<&Rc<BoundOperation>> {
        self.operations.iter().find(|operation| {
            operation.slot == slot
                && operation.requirement.receiver == *receiver
                && operation.requirement.interface == *interface
        })
    }

    pub(crate) fn operation(
        &self,
        required: &NativeCallableRequirement,
    ) -> Option<&Rc<BoundOperation>> {
        self.operations.iter().find(|operation| {
            operation.requirement == *required
                || (operation.generic.is_some()
                    && operation.requirement.receiver == required.receiver
                    && operation.requirement.interface == required.interface
                    && operation.requirement.member == required.member)
        })
    }
}
