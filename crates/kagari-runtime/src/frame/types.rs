//! Reified type arguments retained by a shared frame or closure.
pub mod arguments;
pub(crate) mod compatibility;
pub(crate) mod operations;
use crate::{
    error::RuntimeError,
    frame::types::{
        arguments::TypeArgument,
        operations::{OperationBindings, ReceiverOperations},
    },
    gc::interfaces::{InterfaceResultBinding, MethodApplication},
    module::{LoadedModule, RetainedRuntimeProgram},
};
use kagari_abi::{
    callable::generic::GenericBody,
    native_import::{NativeSignature, callables::NativeCallableRequirement},
    standard::RuntimePrimitive,
    types::{AbiType, GenericParameterAbi, NominalAbiType, substitution::TypeSubstitution},
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::DefinitionPath;
use std::{
    cell::OnceCell,
    rc::{Rc, Weak},
};

#[derive(Debug, Clone)]
pub(crate) struct BoundOperation {
    pub(crate) receiver_operations: Weak<ReceiverOperations>,
    pub(crate) application: OnceCell<Rc<MethodApplication>>,
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
    parameters: Rc<[GenericParameterAbi]>,
    arguments: Rc<[TypeArgument]>,
    parent: Option<Rc<TypeEnvironment>>,
    pub(crate) operations: OperationBindings,
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
            parameters: parameters.into(),
            arguments: arguments.into(),
            parent: None,
            operations: OperationBindings::default(),
        })
    }

    /// Layout metadata cannot keep unrelated executable selections or module state alive.
    pub(crate) fn types_only(&self) -> Self {
        Self {
            parameters: self.parameters.clone(),
            arguments: self.arguments.clone(),
            parent: self
                .parent
                .as_ref()
                .map(|parent| Rc::new(parent.types_only())),
            operations: OperationBindings::default(),
        }
    }

    pub(crate) fn include(&mut self, parent: Option<Rc<Self>>) -> Result<(), RuntimeError> {
        if let Some(parent) = parent {
            if self.parent.is_some()
                || self.parameters.iter().any(|parameter| {
                    parent
                        .argument(&parameter.owner, parameter.position)
                        .is_some()
                })
            {
                return Err(RuntimeError::module_validation(
                    "duplicate generic call binder",
                ));
            }
            self.parent = Some(parent);
        }
        Ok(())
    }

    pub(crate) fn argument(
        &self,
        owner: &DefinitionPath,
        position: usize,
    ) -> Option<&TypeArgument> {
        self.parameters
            .iter()
            .position(|parameter| parameter.owner == *owner && parameter.position == position)
            .and_then(|index| self.arguments.get(index))
            .or_else(|| {
                self.parent
                    .as_ref()
                    .and_then(|parent| parent.argument(owner, position))
            })
    }

    pub(crate) fn matches(&self, body: &GenericBody) -> bool {
        let mut offset = 0;
        let mut environment = Some(self);
        while let Some(current) = environment {
            let end = offset + current.parameters.len();
            if body.parameters.get(offset..end) != Some(current.parameters.as_ref()) {
                return false;
            }
            offset = end;
            environment = current.parent.as_deref();
        }
        offset == body.parameters.len()
    }

    pub(crate) fn resolve(&self, ty: &AbiType) -> Result<AbiType, RuntimeError> {
        let mut substitution = TypeSubstitution::default();
        let mut environment = Some(self);
        while let Some(current) = environment {
            for (parameter, argument) in current.parameters.iter().zip(current.arguments.iter()) {
                substitution.bind(&parameter.owner, parameter.position, argument.ty());
            }
            environment = current.parent.as_deref();
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
        self.operations.operation_slot(receiver, interface, slot)
    }

    pub(crate) fn operation(
        &self,
        required: &NativeCallableRequirement,
    ) -> Option<&Rc<BoundOperation>> {
        self.operations.operation(required)
    }
}
