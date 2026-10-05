//! Immutable lexical type facts, without executable selection or environment edges.
use crate::{error::RuntimeError, frame::types::arguments::TypeArgument, module::LoadedModule};
use kagari_common::identity::{map::DefinitionContext, table::DefinitionId};
use kagari_contract::callable::generic::GenericBody;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{
        GenericParam, NominalTy, Ty,
        substitution::{normalize_projections, substitute_parameters},
    },
};
use std::sync::Arc;

/// Checked associated-type facts; contains no selected operation or environment.
#[derive(Debug, Clone)]
pub(crate) struct AssociatedInterface {
    pub(crate) receiver: Ty<DefinitionId>,
    pub(crate) interface: NominalTy<DefinitionId>,
    pub(crate) owner: LoadedModule,
}

#[derive(Debug, Clone)]
pub(crate) struct TypeBindings {
    definitions: DefinitionContext,
    parameters: Arc<[GenericParam<DefinitionId>]>,
    arguments: Arc<[TypeArgument]>,
    parent: Option<Arc<TypeBindings>>,
    pub(super) associated_interfaces: Arc<Vec<AssociatedInterface>>,
}

impl TypeBindings {
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
            associated_interfaces: Arc::new(vec![]),
        })
    }

    pub(crate) fn include(&mut self, parent: Option<Arc<Self>>) -> Result<(), RuntimeError> {
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
        let result = normalize_projections(
            &result,
            &|interface, receiver, member, arguments| {
                Ok(arguments
                    .is_empty()
                    .then(|| {
                        self.associated_output(receiver, interface, *member)
                            .map(|(ty, _)| ty.clone())
                    })
                    .flatten())
            },
            &Default::default(),
        )
        .map_err(|_| RuntimeError::module_validation("generic associated output"))?;
        if !result.is_concrete() {
            return Err(RuntimeError::module_validation("unbound generic type"));
        }
        Ok(result)
    }

    pub(crate) fn associated_output(
        &self,
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
        member: DefinitionId,
    ) -> Option<(&Ty<DefinitionId>, &LoadedModule)> {
        self.associated_interfaces
            .iter()
            .find_map(|fact| {
                let AssociatedInterface {
                    receiver: ty,
                    interface: applied,
                    owner,
                } = fact;
                (ty == receiver
                    && applied.declaration == interface.declaration
                    && applied.arguments == interface.arguments
                    && interface
                        .associated_types
                        .iter()
                        .all(|(id, ty)| applied.associated_types.get(id) == Some(ty)))
                .then(|| applied.associated_types.get(&member).map(|ty| (ty, owner)))
                .flatten()
            })
            .or_else(|| {
                self.parent
                    .as_ref()
                    .and_then(|parent| parent.associated_output(receiver, interface, member))
            })
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
}
