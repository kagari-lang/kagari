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
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::{map::DefinitionContext, table::DefinitionId};
use kagari_contract::{
    callable::generic::GenericBody, native_import::NativeSignature, standard::RuntimePrimitive,
};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericParam, NominalTy, Ty, substitution::substitute_parameters},
};
use std::{
    cell::OnceCell,
    rc::{Rc, Weak},
};

#[derive(Debug, Clone)]
pub(crate) struct BoundOperation {
    pub(crate) receiver_operations: Weak<ReceiverOperations>,
    pub(crate) application: OnceCell<Rc<MethodApplication>>,
    pub(crate) generic: Option<BoundGenericMethod>,
    pub(crate) requirement: NativeCallableRequirement<DefinitionId>,
    pub(crate) slot: u32,
    pub(crate) primitive: Option<RuntimePrimitive>,
    pub(crate) owner: LoadedModule,
    pub(crate) target: CallableTarget,
    pub(crate) signature: NativeSignature<DefinitionId>,
    pub(crate) retention: Rc<RetainedRuntimeProgram>,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundGenericMethod {
    pub(crate) receiver_table: InterfaceResultBinding,
    pub(crate) receiver_environment: Option<Rc<TypeEnvironment>>,
    pub(crate) parameters: Vec<GenericParam<DefinitionId>>,
    pub(crate) entry_parameters: Vec<GenericParam<DefinitionId>>,
    pub(crate) entry_arguments: Vec<Ty<DefinitionId>>,
}

#[derive(Debug, Clone)]
pub struct TypeEnvironment {
    definitions: DefinitionContext,
    parameters: Rc<[GenericParam<DefinitionId>]>,
    arguments: Rc<[TypeArgument]>,
    parent: Option<Rc<TypeEnvironment>>,
    pub(crate) operations: OperationBindings,
}

impl TypeEnvironment {
    pub(crate) fn new(
        definitions: &DefinitionContext,
        parameters: Vec<GenericParam<DefinitionId>>,
        arguments: Vec<TypeArgument>,
    ) -> Result<Self, RuntimeError> {
        if parameters.len() != arguments.len()
            || !arguments.iter().all(|argument| argument.ty().is_concrete())
        {
            return Err(RuntimeError::module_validation(
                "generic call type arguments",
            ));
        }
        let table = definitions.snapshot();
        for parameter in &parameters {
            table
                .resolve(parameter.owner)
                .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        }
        Ok(Self {
            definitions: definitions.clone(),
            parameters: parameters.into(),
            arguments: arguments.into(),
            parent: None,
            operations: OperationBindings::default(),
        })
    }

    /// Layout metadata cannot keep unrelated executable selections or module state alive.
    pub(crate) fn types_only(&self) -> Self {
        Self {
            definitions: self.definitions.clone(),
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
                || self.definitions.snapshot().id() != parent.definitions.snapshot().id()
                || self.parameters.iter().any(|parameter| {
                    parent
                        .argument_id(parameter.owner, parameter.position)
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

    pub(crate) fn argument(&self, owner: &DefinitionId, position: usize) -> Option<&TypeArgument> {
        self.argument_id(*owner, position)
    }

    fn argument_id(&self, owner: DefinitionId, position: usize) -> Option<&TypeArgument> {
        self.parameters
            .iter()
            .position(|parameter| parameter.owner == owner && parameter.position == position)
            .and_then(|index| self.arguments.get(index))
            .or_else(|| {
                self.parent
                    .as_ref()
                    .and_then(|parent| parent.argument_id(owner, position))
            })
    }

    pub(crate) fn matches(&self, body: &GenericBody<DefinitionId>) -> bool {
        let mut offset = 0;
        let mut environment = Some(self);
        while let Some(current) = environment {
            let end = offset + current.parameters.len();
            let Some(parameters) = body.parameters.get(offset..end) else {
                return false;
            };
            if !parameters
                .iter()
                .zip(current.parameters.iter())
                .all(|(expected, actual)| {
                    expected.owner == actual.owner && expected.position == actual.position
                })
            {
                return false;
            }
            offset = end;
            environment = current.parent.as_deref();
        }
        offset == body.parameters.len()
    }

    pub(crate) fn resolve(&self, ty: &Ty<DefinitionId>) -> Result<Ty<DefinitionId>, RuntimeError> {
        let result = substitute_parameters(
            ty,
            &|owner, position| self.argument(owner, position).map(TypeArgument::ty),
            &Default::default(),
        )
        .map_err(|_| RuntimeError::module_validation("generic type substitution"))?;
        if !result.is_concrete() {
            return Err(RuntimeError::module_validation("unbound generic type"));
        }
        Ok(result)
    }

    pub(crate) fn resolve_requirement(
        &self,
        required: &NativeCallableRequirement<DefinitionId>,
    ) -> Result<NativeCallableRequirement<DefinitionId>, RuntimeError> {
        let Ty::Trait(interface) = self.resolve(&Ty::Trait(required.interface.clone()))? else {
            return Err(RuntimeError::module_validation("constraint interface type"));
        };
        Ok(NativeCallableRequirement {
            receiver: self.resolve(&required.receiver)?,
            interface,
            member: required.member,
            arguments: required
                .arguments
                .iter()
                .map(|ty| self.resolve(ty))
                .collect::<Result<_, _>>()?,
        })
    }

    pub(crate) fn operation_slot(
        &self,
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
        slot: u32,
    ) -> Option<&Rc<BoundOperation>> {
        self.operations.operation_slot(receiver, interface, slot)
    }

    pub(crate) fn operation(
        &self,
        required: &NativeCallableRequirement<DefinitionId>,
    ) -> Option<&Rc<BoundOperation>> {
        self.operations.operation(required)
    }
}
