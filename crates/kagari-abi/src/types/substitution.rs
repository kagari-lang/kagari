//! Bounded binder substitution over executable types, without source inference.
use crate::types::{AbiType, ConstraintAbi, GenericBoundAbi, GenericParameterAbi, NominalAbiType};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};
use std::collections::BTreeMap;

pub(crate) const MAX_TYPE_DEPTH: usize = 64;
pub(crate) const MAX_TYPE_NODES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeTransformError {
    Cancelled,
    LimitExceeded,
    InvalidContract,
}

/// Bindings borrow validated contract records. Substitution replaces exactly one
/// binder layer: a replacement may itself refer to parameters in the caller.
#[derive(Default)]
pub struct TypeSubstitution<'a> {
    parameters: BTreeMap<&'a DefinitionId, BTreeMap<usize, &'a AbiType>>,
    receivers: BTreeMap<&'a DefinitionId, &'a AbiType>,
}

impl<'a> TypeSubstitution<'a> {
    pub fn bind(&mut self, owner: &'a DefinitionId, position: usize, value: &'a AbiType) {
        self.parameters
            .entry(owner)
            .or_default()
            .insert(position, value);
    }

    pub fn bind_receiver(&mut self, owner: &'a DefinitionId, value: &'a AbiType) {
        self.receivers.insert(owner, value);
    }

    pub fn for_owner(owner: &'a DefinitionId, arguments: &'a [AbiType]) -> Self {
        let mut substitution = Self::default();
        for (position, argument) in arguments.iter().enumerate() {
            substitution.bind(owner, position, argument);
        }
        substitution
    }

    pub fn apply(
        &self,
        ty: &AbiType,
        cancel: &CancellationToken,
    ) -> Result<AbiType, TypeTransformError> {
        Transform::new(self, None, cancel).run(ty)
    }

    pub fn apply_bounds(
        &self,
        bounds: &[GenericBoundAbi],
        cancel: &CancellationToken,
    ) -> Result<Vec<GenericBoundAbi>, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if bounds.len() > MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        bounds
            .iter()
            .map(|bound| {
                if bound.constraints.len() > MAX_TYPE_NODES {
                    return Err(TypeTransformError::LimitExceeded);
                }
                Ok(GenericBoundAbi {
                    ty: self.apply(&bound.ty, cancel)?,
                    constraints: bound
                        .constraints
                        .iter()
                        .map(|constraint| {
                            Ok(match constraint {
                                ConstraintAbi::Standard(value) => ConstraintAbi::Standard(*value),
                                ConstraintAbi::Trait(value) => {
                                    let value = self.apply_nominal(value, cancel)?;
                                    ConstraintAbi::Trait(value)
                                }
                            })
                        })
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect()
    }

    pub fn parameter(&self, owner: &DefinitionId, position: usize) -> Option<&'a AbiType> {
        self.parameters.get(owner)?.get(&position).copied()
    }

    pub fn apply_nominal(
        &self,
        ty: &NominalAbiType,
        cancel: &CancellationToken,
    ) -> Result<NominalAbiType, TypeTransformError> {
        let value = AbiType::Trait(Transform::new(self, None, cancel).nominal(ty, 1, true)?);
        if !value.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded);
        }
        let AbiType::Trait(value) = value else {
            unreachable!("nominal type construction")
        };
        Ok(value)
    }

    fn replacement(&self, ty: &AbiType) -> Option<&'a AbiType> {
        match ty {
            AbiType::Parameter { owner, position } => {
                self.parameters.get(owner)?.get(position).copied()
            }
            AbiType::SelfType(owner) => self.receivers.get(owner).copied(),
            _ => None,
        }
    }
}

impl GenericParameterAbi {
    pub fn as_type(&self) -> AbiType {
        AbiType::Parameter {
            owner: self.owner.clone(),
            position: self.position,
        }
    }
}

/// Resolve associated outputs on an applied interface after parameter substitution.
/// Unrelated projections remain symbolic; cyclic or oversized expansions fail.
pub fn resolve_associated_outputs(
    ty: &AbiType,
    interface: &NominalAbiType,
    cancel: &CancellationToken,
) -> Result<AbiType, TypeTransformError> {
    Transform::new(&TypeSubstitution::default(), Some(interface), cancel).run(ty)
}

/// Resolve projections using a caller's already validated dependency contracts.
/// A missing binding stays symbolic; malformed, ambiguous or unbounded proofs fail.
pub type ProjectionLookup<'a> = dyn Fn(
        &NominalAbiType,
        &AbiType,
        &DefinitionId,
        &[AbiType],
    ) -> Result<Option<AbiType>, TypeTransformError>
    + 'a;

pub fn normalize_projections(
    ty: &AbiType,
    lookup: &ProjectionLookup<'_>,
    cancel: &CancellationToken,
) -> Result<AbiType, TypeTransformError> {
    let substitution = TypeSubstitution::default();
    let mut transform = Transform::new(&substitution, None, cancel);
    transform.lookup = Some(lookup);
    transform.run(ty)
}

struct Transform<'a, 'b> {
    substitution: &'a TypeSubstitution<'b>,
    outputs: Option<&'a NominalAbiType>,
    lookup: Option<&'a ProjectionLookup<'a>>,
    cancel: &'a CancellationToken,
    remaining: usize,
}

impl<'a, 'b> Transform<'a, 'b> {
    fn new(
        substitution: &'a TypeSubstitution<'b>,
        outputs: Option<&'a NominalAbiType>,
        cancel: &'a CancellationToken,
    ) -> Self {
        Self {
            substitution,
            outputs,
            lookup: None,
            cancel,
            remaining: MAX_TYPE_NODES * 4,
        }
    }

    fn step(&mut self, depth: usize) -> Result<(), TypeTransformError> {
        self.cancel
            .check()
            .map_err(|_| TypeTransformError::Cancelled)?;
        if depth > MAX_TYPE_DEPTH || self.remaining == 0 {
            return Err(TypeTransformError::LimitExceeded);
        }
        self.remaining -= 1;
        Ok(())
    }

    fn id(&self, id: &DefinitionId) -> Result<DefinitionId, TypeTransformError> {
        if !id.within_path_limit() {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(id.clone())
    }

    fn run(&mut self, ty: &AbiType) -> Result<AbiType, TypeTransformError> {
        let result = self.visit(ty, 1, true)?;
        if !result.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(result)
    }

    fn many(
        &mut self,
        types: &[AbiType],
        depth: usize,
        replace: bool,
    ) -> Result<Vec<AbiType>, TypeTransformError> {
        if types.len() > MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        types
            .iter()
            .map(|ty| self.visit(ty, depth, replace))
            .collect()
    }

    fn nominal(
        &mut self,
        ty: &NominalAbiType,
        depth: usize,
        replace: bool,
    ) -> Result<NominalAbiType, TypeTransformError> {
        self.step(depth)?;
        if ty.associated_types.len() > MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(NominalAbiType {
            declaration: self.id(&ty.declaration)?,
            arguments: self.many(&ty.arguments, depth + 1, replace)?,
            associated_types: ty
                .associated_types
                .iter()
                .map(|(id, ty)| Ok((self.id(id)?, self.visit(ty, depth + 1, replace)?)))
                .collect::<Result<_, _>>()?,
        })
    }

    fn visit(
        &mut self,
        ty: &AbiType,
        depth: usize,
        replace: bool,
    ) -> Result<AbiType, TypeTransformError> {
        self.step(depth)?;
        if replace && let Some(replacement) = self.substitution.replacement(ty) {
            // Do not feed a caller's parameters back into this substitution.
            return self.visit(replacement, depth, false);
        }
        Ok(match ty {
            AbiType::Builtin(kind) => AbiType::Builtin(*kind),
            AbiType::Host(id) => AbiType::Host(self.id(id)?),
            AbiType::SelfType(id) => AbiType::SelfType(self.id(id)?),
            AbiType::Parameter { owner, position } => AbiType::Parameter {
                owner: self.id(owner)?,
                position: *position,
            },
            AbiType::Tuple(types) => AbiType::Tuple(self.many(types, depth + 1, replace)?),
            AbiType::Function { params, result } => AbiType::Function {
                params: self.many(params, depth + 1, replace)?,
                result: Box::new(self.visit(result, depth + 1, replace)?),
            },
            AbiType::Iter(ty) => AbiType::Iter(Box::new(self.visit(ty, depth + 1, replace)?)),
            AbiType::Range(ty, kind) => {
                AbiType::Range(Box::new(self.visit(ty, depth + 1, replace)?), *kind)
            }
            AbiType::Array(ty, access) => {
                AbiType::Array(Box::new(self.visit(ty, depth + 1, replace)?), *access)
            }
            AbiType::Set(ty, access) => {
                AbiType::Set(Box::new(self.visit(ty, depth + 1, replace)?), *access)
            }
            AbiType::Map { key, value, access } => AbiType::Map {
                key: Box::new(self.visit(key, depth + 1, replace)?),
                value: Box::new(self.visit(value, depth + 1, replace)?),
                access: *access,
            },
            AbiType::StandardEnum { kind, args } => AbiType::StandardEnum {
                kind: *kind,
                args: self.many(args, depth + 1, replace)?,
            },
            AbiType::Struct(ty) => AbiType::Struct(self.nominal(ty, depth, replace)?),
            AbiType::Enum(ty) => AbiType::Enum(self.nominal(ty, depth, replace)?),
            AbiType::Trait(ty) => AbiType::Trait(self.nominal(ty, depth, replace)?),
            AbiType::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                let receiver = self.visit(receiver, depth + 1, replace)?;
                let interface = self.nominal(interface, depth, replace)?;
                let arguments = self.many(arguments, depth + 1, replace)?;
                if self.outputs.is_some() || self.lookup.is_some() {
                    if arguments.is_empty() {
                        let embedded = interface.associated_types.get(member).or_else(|| {
                            if let AbiType::Trait(actual) = &receiver {
                                actual.associated_types.get(member)
                            } else {
                                None
                            }
                        });
                        if let Some(output) = embedded {
                            return self.visit(output, depth + 1, replace);
                        }
                        if let Some(outputs) = self.outputs
                            && interface.declaration == outputs.declaration
                            && let Some(output) = outputs.associated_types.get(member)
                        {
                            return self.visit(output, depth + 1, replace);
                        }
                    }
                    if let Some(lookup) = self.lookup
                        && let Some(output) = lookup(&interface, &receiver, member, &arguments)?
                    {
                        return self.visit(&output, depth + 1, replace);
                    }
                }
                AbiType::Projection {
                    receiver: Box::new(receiver),
                    interface: Box::new(interface),
                    member: self.id(member)?,
                    arguments,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
