//! Installed conversion adapters retain exact forward method and error identities.
use crate::{
    declaration::TraitDef,
    ty::{NominalTy, Ty},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionAdapter {
    /// A source receiver is adapted through the destination's checked impl.
    Reverse {
        origin: DefinitionId,
        method: DefinitionId,
        error: Option<(DefinitionId, DefinitionId)>,
        result: Option<DefinitionId>,
    },
    /// A checked forward method/result contract; ordinary implementations own
    /// associated error selection and numeric evaluation.
    Forward {
        method: DefinitionId,
        error: DefinitionId,
        result: DefinitionId,
    },
}

impl RemapDefinitions for ConversionAdapter {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_, DefinitionId, DefinitionId>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Reverse {
                origin,
                method,
                error,
                result,
            } => ConversionAdapter::Reverse {
                origin: mapper.reference(origin)?,
                method: mapper.reference(method)?,
                error: error
                    .as_ref()
                    .map(|(source, target)| {
                        Ok::<_, DefinitionMappingError>((
                            mapper.reference(source)?,
                            mapper.reference(target)?,
                        ))
                    })
                    .transpose()?,
                result: result.as_ref().map(|id| mapper.reference(id)).transpose()?,
            },
            Self::Forward {
                method,
                error,
                result,
            } => ConversionAdapter::Forward {
                method: mapper.reference(method)?,
                error: mapper.reference(error)?,
                result: mapper.reference(result)?,
            },
        })
    }
}

impl DefinitionReferences for ConversionAdapter {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Reverse {
                origin,
                method,
                error,
                result,
            } => {
                visit(origin)?;
                visit(method)?;
                if let Some((source, target)) = error {
                    visit(source)?;
                    visit(target)?;
                }
                if let Some(result) = result {
                    visit(result)?;
                }
            }
            Self::Forward {
                method,
                error,
                result,
            } => {
                visit(method)?;
                visit(error)?;
                visit(result)?;
            }
        }
        Ok(())
    }
}

impl ConversionAdapter {
    /// Validate the adapter's local declaration shape before linked proofs use it.
    pub fn valid_in(
        &self,
        owner: &DefinitionId,
        contract: &TraitDef,
        definitions: &DefinitionTable,
    ) -> bool {
        if contract.storage_access.is_some()
            || contract.generic_params.len() != 1
            || contract.methods.len() != 1
            || !contract.associated_consts.is_empty()
        {
            return false;
        }
        let member_of = |id: &DefinitionId, parent: &DefinitionId, kind| {
            definitions.parent(*id) == Ok(Some(*parent))
                && definitions
                    .resolve(*id)
                    .ok()
                    .and_then(|view| view.last())
                    .is_some_and(|member| {
                        member.kind == kind && member.occurrence == 0 && !member.name.is_empty()
                    })
        };
        let parameter = contract.generic_params[0].as_type();
        let this = Ty::SelfType(*owner);
        let (input, output, error) = match self {
            Self::Reverse {
                origin,
                method,
                error,
                result,
            } => {
                if result.is_some() != error.is_some()
                    || origin == owner
                    || !definitions.resolve(*origin).is_ok_and(|view| {
                        view.segments().count() == 1
                            && view.last().is_some_and(|part| {
                                part.kind == DefinitionKind::Trait
                                    && part.occurrence == 0
                                    && !part.name.is_empty()
                            })
                    })
                    || !member_of(method, origin, DefinitionKind::Method)
                    || error.as_ref().is_some_and(|(_, target)| {
                        !member_of(target, origin, DefinitionKind::AssociatedType)
                    })
                {
                    return false;
                }
                (
                    this.clone(),
                    parameter.clone(),
                    error.as_ref().map(|(source, _)| source),
                )
            }
            Self::Forward { method, error, .. } => {
                if !member_of(method, owner, DefinitionKind::Method)
                    || definitions
                        .resolve(*method)
                        .ok()
                        .and_then(|view| view.last())
                        .map(|part| part.name)
                        != Some(contract.methods[0].name.as_str())
                {
                    return false;
                }
                (parameter.clone(), this.clone(), Some(error))
            }
        };
        let output = if let Some(error) = error {
            if contract.associated_types.len() != 1
                || contract.associated_types[0].declaration != *error
                || !contract.associated_types[0].generic_params.is_empty()
            {
                return false;
            }
            let declaration = match self {
                Self::Reverse {
                    result: Some(result),
                    ..
                }
                | Self::Forward { result, .. } => *result,
                _ => return false,
            };
            Ty::Enum(NominalTy {
                declaration,
                associated_types: Default::default(),
                arguments: vec![
                    output,
                    Ty::Projection {
                        receiver: Box::new(this),
                        interface: Box::new(NominalTy {
                            declaration: *owner,
                            arguments: vec![parameter],
                            associated_types: Default::default(),
                        }),
                        member: *error,
                        arguments: vec![],
                    },
                ],
            })
        } else {
            if !contract.associated_types.is_empty() {
                return false;
            }
            output
        };
        let method = &contract.methods[0];
        method.generic_params.is_empty()
            && method.params.len() == 1
            && method.params[0].ty == input
            && method.return_type == output
    }
}

impl ConversionAdapter {
    pub fn reverse_requirement(
        &self,
        interface: &NominalTy,
        receiver: &Ty,
    ) -> Option<(NominalTy, Ty)> {
        let Self::Reverse { origin, error, .. } = self else {
            return None;
        };
        let [target] = interface.arguments.as_slice() else {
            return None;
        };
        let mut required = NominalTy {
            declaration: *origin,
            arguments: vec![receiver.clone()],
            associated_types: Default::default(),
        };
        for (member, ty) in &interface.associated_types {
            let (source, target) = error.as_ref()?;
            if member != source {
                return None;
            }
            required.associated_types.insert(*target, ty.clone());
        }
        Some((required, target.clone()))
    }

    pub fn associated_output(
        &self,
        interface: &NominalTy,
        receiver: &Ty,
        member: &DefinitionId,
    ) -> Option<Ty> {
        let Self::Reverse {
            origin,
            error: Some((source, target)),
            ..
        } = self
        else {
            return None;
        };
        let [destination] = interface.arguments.as_slice() else {
            return None;
        };
        (member == source && !matches!(receiver, Ty::SelfType(_))).then(|| Ty::Projection {
            receiver: Box::new(destination.clone()),
            interface: Box::new(NominalTy {
                declaration: *origin,
                arguments: vec![receiver.clone()],
                associated_types: Default::default(),
            }),
            member: *target,
            arguments: vec![],
        })
    }
}
