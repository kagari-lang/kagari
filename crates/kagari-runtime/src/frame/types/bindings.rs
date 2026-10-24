//! Immutable lexical type facts, without executable selection or environment edges.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{EnvironmentRecord, arguments::TypeArgument, compatibility::TypeView},
    module::{
        ExecutableModule,
        dispatch::{
            SelectedTarget,
            identity::{PublicationDefinitions, PublicationMapping},
        },
    },
};
use kagari_common::identity::{map::DefinitionContext, table::DefinitionId};
use kagari_contract::callable::generic::GenericBody;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{
        GenericParam, NominalTy, Ty,
        substitution::{normalize_projections, substitute_parameters},
    },
};
use std::{collections::BTreeMap, slice, sync::Arc};

/// Checked associated-type facts; contains no selected operation or environment.
#[derive(Debug, Clone)]
pub(crate) struct AssociatedInterface {
    pub(crate) lookup_scope: Option<Arc<PublicationDefinitions>>,
    pub(crate) receiver: Ty,
    pub(crate) interface: NominalTy,
    pub(crate) owner: ExecutableModule,
    pub(crate) outputs: BTreeMap<DefinitionId, TypeArgument>,
}

#[derive(Debug, Clone)]
pub(crate) struct TypeBindings {
    definitions: DefinitionContext,
    parameters: Arc<[GenericParam]>,
    arguments: Arc<[TypeArgument]>,
    parent: Option<Arc<TypeBindings>>,
    pub(super) associated_interfaces: Arc<Vec<AssociatedInterface>>,
}

impl AssociatedInterface {
    pub(crate) fn transported(
        &self,
        runtime: &Runtime,
        selected: &SelectedTarget,
    ) -> Result<Self, RuntimeError> {
        if self.owner.installation_id() != selected.owner().installation_id() {
            if !self
                .owner
                .is_shared_native_trait(self.interface.declaration)
            {
                return Err(RuntimeError::module_validation(
                    "script trait output belongs to another installation",
                ));
            }
            return Ok(self.clone());
        }
        let mapping = selected.mapping_from(
            self.lookup_scope
                .clone()
                .unwrap_or_else(|| self.owner.publication_definitions()),
        )?;
        let mut outputs = BTreeMap::new();
        for (member, ty) in &self.interface.associated_types {
            let output = match self.outputs.get(member) {
                Some(output) => output.clone(),
                None => runtime
                    .type_arguments(&self.owner, None, slice::from_ref(ty))?
                    .pop()
                    .ok_or_else(|| RuntimeError::module_validation("associated output scope"))?,
            };
            outputs.insert(mapping.map(*member)?, output);
        }
        for (member, output) in &self.outputs {
            outputs.insert(mapping.map(*member)?, output.clone());
        }
        Ok(Self {
            lookup_scope: Some(selected.owner().publication_definitions()),
            receiver: mapping
                .remap(&self.receiver, &Default::default())?
                .into_records(),
            interface: mapping
                .remap(&self.interface, &Default::default())?
                .into_records(),
            owner: self.owner.clone(),
            outputs,
        })
    }
}

impl EnvironmentRecord {
    pub(crate) fn transported(
        &self,
        runtime: &Runtime,
        selected: &SelectedTarget,
        mapping: &PublicationMapping,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            types: Arc::new(self.types.transported(runtime, selected, mapping)?),
            constructions: self.constructions.transported(runtime, selected)?,
            parent: self.parent,
            operations: self.operations.transported(runtime, selected)?,
        })
    }
}

impl TypeBindings {
    fn transported(
        &self,
        runtime: &Runtime,
        selected: &SelectedTarget,
        mapping: &PublicationMapping,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            definitions: self.definitions.clone(),
            parameters: mapping
                .remap(&self.parameters.to_vec(), &Default::default())?
                .into_records()
                .into(),
            arguments: self.arguments.clone(),
            parent: self
                .parent
                .as_ref()
                .map(|parent| parent.transported(runtime, selected, mapping).map(Arc::new))
                .transpose()?,
            associated_interfaces: Arc::new(
                self.associated_interfaces
                    .iter()
                    .map(|fact| fact.transported(runtime, selected))
                    .collect::<Result<_, _>>()?,
            ),
        })
    }

    pub(crate) fn new(
        definitions: &DefinitionContext,
        parameters: Vec<GenericParam>,
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

    pub(crate) fn matches(&self, body: &GenericBody) -> bool {
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

    pub(crate) fn resolve(&self, ty: &Ty) -> Result<Ty, RuntimeError> {
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
                            .and_then(|view| view.closed().map(|ty| ty.into_owned()))
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
        receiver: &Ty,
        interface: &NominalTy,
        member: DefinitionId,
    ) -> Option<TypeView<'_>> {
        self.associated_interfaces
            .iter()
            .find_map(|fact| {
                let AssociatedInterface {
                    receiver: ty,
                    interface: applied,
                    owner,
                    outputs,
                    ..
                } = fact;
                (ty == receiver
                    && applied.declaration == interface.declaration
                    && applied.arguments == interface.arguments
                    && interface
                        .associated_types
                        .iter()
                        .all(|(id, ty)| applied.associated_types.get(id) == Some(ty)))
                .then(|| {
                    outputs
                        .get(&member)
                        .map(|argument| argument.view(owner))
                        .or_else(|| {
                            applied
                                .associated_types
                                .get(&member)
                                .map(|ty| TypeView::new(ty, owner, None))
                        })
                })
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
        required: &NativeCallableRequirement,
    ) -> Result<NativeCallableRequirement, RuntimeError> {
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
