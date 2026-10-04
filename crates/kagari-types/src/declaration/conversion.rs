//! Installed conversion adapters retain exact forward method and error identities.
use crate::{
    declaration::TraitDef,
    ty::{NominalTy, Ty},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath,
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel},
        reference::DefinitionReference,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + Serialize",
    deserialize = "I: DefinitionReference + Deserialize<'de>"
))]
pub enum ConversionAdapter<I = DefinitionPath> {
    /// A source receiver is adapted through the destination's checked impl.
    Reverse {
        origin: I,
        method: I,
        error: Option<(I, I)>,
        result: Option<I>,
    },
    /// A checked forward method/result contract; ordinary implementations own
    /// associated error selection and numeric evaluation.
    Forward { method: I, error: I, result: I },
}

impl<I: DefinitionReference> DefinitionRecord<I> for ConversionAdapter<I> {
    type Rebind<J: DefinitionReference> = ConversionAdapter<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
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

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
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
    pub fn valid_in(&self, owner: &DefinitionPath, contract: &TraitDef) -> bool {
        if contract.storage_access.is_some()
            || contract.generic_params.len() != 1
            || contract.methods.len() != 1
            || !contract.associated_consts.is_empty()
        {
            return false;
        }
        let member_of = |id: &DefinitionPath, parent: &DefinitionPath, kind| {
            let mut actual = id.clone();
            actual.path.pop().is_some_and(|member| {
                member.kind == kind && member.occurrence == 0 && !member.name.is_empty()
            }) && actual == *parent
                && id.within_path_limit()
        };
        let parameter = contract.generic_params[0].as_type();
        let this = Ty::SelfType(owner.clone());
        let (input, output, error) = match self {
            Self::Reverse {
                origin,
                method,
                error,
                result,
            } => {
                if result.is_some() != error.is_some()
                    || origin == owner
                    || origin.path.len() != 1
                    || origin.path[0].kind != DefinitionKind::Trait
                    || origin.path[0].occurrence != 0
                    || origin.path[0].name.is_empty()
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
                    || method.path.last().map(|part| &part.name) != Some(&contract.methods[0].name)
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
                | Self::Forward { result, .. } => result.clone(),
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
                            declaration: owner.clone(),
                            arguments: vec![parameter],
                            associated_types: Default::default(),
                        }),
                        member: error.clone(),
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
            declaration: origin.clone(),
            arguments: vec![receiver.clone()],
            associated_types: Default::default(),
        };
        for (member, ty) in &interface.associated_types {
            let (source, target) = error.as_ref()?;
            if member != source {
                return None;
            }
            required.associated_types.insert(target.clone(), ty.clone());
        }
        Some((required, target.clone()))
    }

    pub fn associated_output(
        &self,
        interface: &NominalTy,
        receiver: &Ty,
        member: &DefinitionPath,
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
                declaration: origin.clone(),
                arguments: vec![receiver.clone()],
                associated_types: Default::default(),
            }),
            member: target.clone(),
            arguments: vec![],
        })
    }
}
