//! Bounded binder substitution over executable types, without source inference.
use crate::ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};
use std::collections::BTreeMap;

pub const MAX_TYPE_DEPTH: usize = 64;
pub const MAX_TYPE_NODES: usize = 4_096;

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
    parameters: BTreeMap<&'a DefinitionId, BTreeMap<usize, &'a Ty>>,
    receivers: BTreeMap<&'a DefinitionId, &'a Ty>,
}

impl<'a> TypeSubstitution<'a> {
    pub fn bind(&mut self, owner: &'a DefinitionId, position: usize, value: &'a Ty) {
        self.parameters
            .entry(owner)
            .or_default()
            .insert(position, value);
    }

    pub fn bind_receiver(&mut self, owner: &'a DefinitionId, value: &'a Ty) {
        self.receivers.insert(owner, value);
    }

    pub fn for_owner(owner: &'a DefinitionId, arguments: &'a [Ty]) -> Self {
        let mut substitution = Self::default();
        for (position, argument) in arguments.iter().enumerate() {
            substitution.bind(owner, position, argument);
        }
        substitution
    }

    pub fn apply(&self, ty: &Ty, cancel: &CancellationToken) -> Result<Ty, TypeTransformError> {
        Transform::new(self, None, cancel).run(ty)
    }

    /// Substitute one binder layer and merge bounds in canonical semantic order.
    /// The table must contain all resulting identities. Foreign or stale references,
    /// excessive type depth/node counts and cancellation return an error.
    pub fn apply_bounds(
        &self,
        bounds: &[GenericBound],
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Vec<GenericBound>, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if bounds.len() > MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        let applied = bounds
            .iter()
            .map(|bound| {
                if bound.constraints.len() > MAX_TYPE_NODES {
                    return Err(TypeTransformError::LimitExceeded);
                }
                Ok(GenericBound {
                    ty: self.apply(&bound.ty, cancel)?,
                    constraints: bound
                        .constraints
                        .iter()
                        .map(|constraint| {
                            Ok(match constraint {
                                Constraint::Standard(value) => Constraint::Standard(*value),
                                Constraint::Trait(value) => {
                                    let value = self.apply_nominal(value, cancel)?;
                                    Constraint::Trait(value)
                                }
                            })
                        })
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<Vec<_>, TypeTransformError>>()?;
        // Substitution can reorder receivers or collapse distinct binder subjects.
        // The scoped normalizer owns canonical ordering and constraint merging.
        GenericBound::normalize(applied, definitions, cancel)
    }

    pub fn parameter(&self, owner: &DefinitionId, position: usize) -> Option<&'a Ty> {
        self.parameters.get(owner)?.get(&position).copied()
    }

    pub fn apply_nominal(
        &self,
        ty: &NominalTy,
        cancel: &CancellationToken,
    ) -> Result<NominalTy, TypeTransformError> {
        let value = Ty::Trait(Transform::new(self, None, cancel).nominal(ty, 1, true)?);
        if !value.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded);
        }
        let Ty::Trait(value) = value else {
            unreachable!("nominal type construction")
        };
        Ok(value)
    }

    fn replacement(&self, ty: &Ty) -> Option<&'a Ty> {
        match ty {
            Ty::Parameter { owner, position } => self.parameters.get(owner)?.get(position).copied(),
            Ty::SelfType(owner) => self.receivers.get(owner).copied(),
            _ => None,
        }
    }
}

impl GenericParam {
    pub fn as_type(&self) -> Ty {
        Ty::Parameter {
            owner: self.owner,
            position: self.position,
        }
    }
}

/// Resolve associated outputs on an applied interface after parameter substitution.
/// Unrelated projections remain symbolic; cyclic or oversized expansions fail.
pub fn resolve_associated_outputs(
    ty: &Ty,
    interface: &NominalTy,
    receiver: &Ty,
    cancel: &CancellationToken,
) -> Result<Ty, TypeTransformError> {
    Transform::new(
        &TypeSubstitution::default(),
        Some((interface, receiver)),
        cancel,
    )
    .run(ty)
}

/// Resolve projections using a caller's already validated dependency contracts.
/// A missing binding stays symbolic; malformed, ambiguous or unbounded proofs fail.
pub type ProjectionLookup<'a> =
    dyn Fn(&NominalTy, &Ty, &DefinitionId, &[Ty]) -> Result<Option<Ty>, TypeTransformError> + 'a;

pub fn normalize_projections(
    ty: &Ty,
    lookup: &ProjectionLookup<'_>,
    cancel: &CancellationToken,
) -> Result<Ty, TypeTransformError> {
    let substitution = TypeSubstitution::default();
    let mut transform = Transform::new(&substitution, None, cancel);
    transform.lookup = Some(lookup);
    transform.run(ty)
}

type ParameterLookup<'a> = dyn Fn(&DefinitionId, usize) -> Option<&'a Ty> + 'a;

/// Apply the same bounded, once-only substitution using an explicit contextual
/// binder lookup. Runtime frames can retain compact owners without rebuilding
/// owned-path maps for every substitution.
pub fn substitute_parameters<'a>(
    ty: &Ty,
    lookup: &ParameterLookup<'a>,
    cancel: &CancellationToken,
) -> Result<Ty, TypeTransformError> {
    let substitution = TypeSubstitution::default();
    let mut transform = Transform::new(&substitution, None, cancel);
    transform.parameter_lookup = Some(lookup);
    transform.run(ty)
}

struct Transform<'a, 'b> {
    substitution: &'a TypeSubstitution<'b>,
    outputs: Option<(&'a NominalTy, &'a Ty)>,
    lookup: Option<&'a ProjectionLookup<'a>>,
    parameter_lookup: Option<&'a ParameterLookup<'b>>,
    cancel: &'a CancellationToken,
    remaining: usize,
}

impl<'a, 'b> Transform<'a, 'b> {
    fn new(
        substitution: &'a TypeSubstitution<'b>,
        outputs: Option<(&'a NominalTy, &'a Ty)>,
        cancel: &'a CancellationToken,
    ) -> Self {
        Self {
            substitution,
            outputs,
            lookup: None,
            parameter_lookup: None,
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

    fn run(&mut self, ty: &Ty) -> Result<Ty, TypeTransformError> {
        let result = self.visit(ty, 1, true)?;
        if !result.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(result)
    }

    fn many(
        &mut self,
        types: &[Ty],
        depth: usize,
        replace: bool,
    ) -> Result<Vec<Ty>, TypeTransformError> {
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
        ty: &NominalTy,
        depth: usize,
        replace: bool,
    ) -> Result<NominalTy, TypeTransformError> {
        self.step(depth)?;
        if ty.associated_types.len() > MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(NominalTy {
            declaration: ty.declaration,
            arguments: self.many(&ty.arguments, depth + 1, replace)?,
            associated_types: ty
                .associated_types
                .iter()
                .map(|(id, ty)| Ok((*id, self.visit(ty, depth + 1, replace)?)))
                .collect::<Result<_, _>>()?,
        })
    }

    fn visit(&mut self, ty: &Ty, depth: usize, replace: bool) -> Result<Ty, TypeTransformError> {
        self.step(depth)?;
        if replace {
            let replacement = if let Ty::Parameter { owner, position } = ty
                && let Some(lookup) = self.parameter_lookup
            {
                lookup(owner, *position)
            } else {
                self.substitution.replacement(ty)
            };
            if let Some(replacement) = replacement {
                // Do not feed a caller's parameters back into this substitution.
                return self.visit(replacement, depth, false);
            }
        }
        Ok(match ty {
            Ty::Builtin(kind) => Ty::Builtin(*kind),
            Ty::Host(id) => Ty::Host(*id),
            Ty::SelfType(id) => Ty::SelfType(*id),
            Ty::Parameter { owner, position } => Ty::Parameter {
                owner: *owner,
                position: *position,
            },
            Ty::Tuple(types) => Ty::Tuple(self.many(types, depth + 1, replace)?),
            Ty::Function { params, result } => Ty::Function {
                params: self.many(params, depth + 1, replace)?,
                result: Box::new(self.visit(result, depth + 1, replace)?),
            },
            Ty::Iter(ty) => Ty::Iter(Box::new(self.visit(ty, depth + 1, replace)?)),
            Ty::Range(ty, kind) => Ty::Range(Box::new(self.visit(ty, depth + 1, replace)?), *kind),
            Ty::Array(ty) => Ty::Array(Box::new(self.visit(ty, depth + 1, replace)?)),
            Ty::Set(ty, access) => Ty::Set(Box::new(self.visit(ty, depth + 1, replace)?), *access),
            Ty::Map { key, value, access } => Ty::Map {
                key: Box::new(self.visit(key, depth + 1, replace)?),
                value: Box::new(self.visit(value, depth + 1, replace)?),
                access: *access,
            },

            Ty::Struct(ty) => Ty::Struct(self.nominal(ty, depth, replace)?),
            Ty::NativeObject(ty) => Ty::NativeObject(self.nominal(ty, depth, replace)?),
            Ty::Enum(ty) => Ty::Enum(self.nominal(ty, depth, replace)?),
            Ty::Trait(ty) => Ty::Trait(self.nominal(ty, depth, replace)?),
            Ty::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                let receiver = self.visit(receiver, depth + 1, replace)?;
                let mut interface = self.nominal(interface, depth, replace)?;
                let arguments = self.many(arguments, depth + 1, replace)?;
                if self.outputs.is_some() || self.lookup.is_some() {
                    if arguments.is_empty() {
                        let embedded = interface.associated_types.get(member).or_else(|| {
                            if let Ty::Trait(actual) = &receiver {
                                actual.associated_types.get(member)
                            } else {
                                None
                            }
                        });
                        if let Some(output) = embedded {
                            return self.visit(output, depth + 1, replace);
                        }
                        if let Some((outputs, owner)) = self.outputs
                            && interface.declaration == outputs.declaration
                            && &receiver == owner
                        {
                            if let Some(output) = outputs.associated_types.get(member) {
                                return self.visit(output, depth + 1, replace);
                            }
                            interface = self.nominal(outputs, depth, replace)?;
                        }
                    }
                    if let Some(lookup) = self.lookup
                        && let Some(output) = lookup(&interface, &receiver, member, &arguments)?
                    {
                        return self.visit(&output, depth + 1, replace);
                    }
                }
                Ty::Projection {
                    receiver: Box::new(receiver),
                    interface: Box::new(interface),
                    member: *member,
                    arguments,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
