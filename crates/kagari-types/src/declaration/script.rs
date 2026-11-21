//! Ordinary expression bodies supplied by structured declaration producers.
use crate::declaration::verify::{Parameters, type_valid};
use crate::ty::{
    NominalTy, Ty,
    substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
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

/// A tail expression in a Script definition. Calls carry explicit argument
/// expressions; no implementation selector or default expansion is involved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptExpr {
    Name(String),
    Definition(DefinitionId),
    /// `<receiver as interface>::member`, only as the callee of an ordinary call.
    /// `member` identifies the trait declaration's method, not an implementation.
    TraitMethod {
        receiver: Ty,
        interface: NominalTy,
        member: DefinitionId,
    },
    Call {
        callee: Box<ScriptExpr>,
        type_arguments: Vec<Ty>,
        arguments: Vec<ScriptExpr>,
    },
}

impl RemapDefinitions for ScriptExpr {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        self.validate_shape(&CancellationToken::default())?;
        self.remap_checked(mapper)
    }
}

impl ScriptExpr {
    fn remap_checked(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(match self {
            Self::Name(name) => Self::Name(name.clone()),
            Self::Definition(id) => Self::Definition(mapper.reference(id)?),
            Self::TraitMethod {
                receiver,
                interface,
                member,
            } => Self::TraitMethod {
                receiver: receiver.remap_definitions(mapper)?,
                interface: interface.remap_definitions(mapper)?,
                member: mapper.reference(member)?,
            },
            Self::Call {
                callee,
                type_arguments,
                arguments,
            } => Self::Call {
                callee: Box::new(callee.remap_checked(mapper)?),
                type_arguments: type_arguments
                    .iter()
                    .map(|ty| ty.remap_definitions(mapper))
                    .collect::<Result<_, _>>()?,
                arguments: arguments
                    .iter()
                    .map(|expr| expr.remap_checked(mapper))
                    .collect::<Result<_, _>>()?,
            },
        })
    }
}

impl DefinitionReferences for ScriptExpr {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.validate_shape(cancel)?;
        let mut pending = vec![self];
        while let Some(expression) = pending.pop() {
            check_cancel(cancel)?;
            match expression {
                Self::Name(_) => {}
                Self::Definition(id) => visit(id)?,
                Self::TraitMethod {
                    receiver,
                    interface,
                    member,
                } => {
                    receiver.visit_definition_ids(visit, cancel)?;
                    interface.visit_definition_ids(visit, cancel)?;
                    visit(member)?;
                }
                Self::Call {
                    callee,
                    type_arguments,
                    arguments,
                } => {
                    for ty in type_arguments {
                        ty.visit_definition_ids(visit, cancel)?;
                    }
                    pending.push(callee);
                    pending.extend(arguments);
                }
            }
        }
        Ok(())
    }
}

impl ScriptExpr {
    /// Validate expression depth and total shape before recursive consumers run.
    pub fn validate_shape(&self, cancel: &CancellationToken) -> Result<(), DefinitionMappingError> {
        self.walk(cancel, |_| Ok(()))
    }

    pub(super) fn validate_types(
        &self,
        params: &Parameters,
        self_owner: Option<&DefinitionId>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        let valid = |ty: &Ty| {
            ty.within_wire_limits() && type_valid(ty, params, self_owner, cancel, definitions)
        };
        self.walk(cancel, |expr| {
            let ok = match expr {
                Self::TraitMethod {
                    receiver,
                    interface,
                    member,
                } => {
                    let method = definitions.resolve(*member)?;
                    method.last().is_some_and(|part| {
                        part.kind == DefinitionKind::Method && part.occurrence == 0
                    }) && definitions.parent(*member)? == Some(interface.declaration)
                        && valid(receiver)
                        && valid(&Ty::Trait(interface.clone()))
                }
                Self::Call { type_arguments, .. } => type_arguments.iter().all(valid),
                _ => true,
            };
            ok.then_some(())
                .ok_or(DefinitionMappingError::InvalidContract)
        })
    }

    fn walk(
        &self,
        cancel: &CancellationToken,
        mut check: impl FnMut(&Self) -> Result<(), DefinitionMappingError>,
    ) -> Result<(), DefinitionMappingError> {
        let mut pending = vec![(self, 0, false)];
        let mut nodes = 0usize;
        while let Some((expression, depth, is_callee)) = pending.pop() {
            check_cancel(cancel)?;
            nodes += 1;
            if depth > MAX_TYPE_DEPTH || nodes > MAX_TYPE_NODES {
                return Err(DefinitionMappingError::InvalidContract);
            }
            match expression {
                Self::TraitMethod {
                    receiver,
                    interface,
                    ..
                } if !is_callee
                    || !receiver.within_wire_limits()
                    || !interface.within_wire_limits(cancel) =>
                {
                    return Err(DefinitionMappingError::InvalidContract);
                }
                Self::Name(name) if name.is_empty() => {
                    return Err(DefinitionMappingError::InvalidContract);
                }
                Self::Call {
                    callee,
                    type_arguments,
                    arguments,
                } => {
                    if type_arguments.len() > MAX_TYPE_NODES || arguments.len() > MAX_TYPE_NODES {
                        return Err(DefinitionMappingError::InvalidContract);
                    }
                    pending.push((callee, depth + 1, true));
                    pending.extend(
                        arguments
                            .iter()
                            .map(|argument| (argument, depth + 1, false)),
                    );
                }
                _ => {}
            }
            check(expression)?;
        }
        Ok(())
    }
}
