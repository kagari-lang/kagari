//! Bounded binder substitution over executable types, without source inference.
use crate::ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, reference::DefinitionReference},
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
pub struct TypeSubstitution<'a, I = DefinitionPath> {
    parameters: BTreeMap<&'a I, BTreeMap<usize, &'a Ty<I>>>,
    receivers: BTreeMap<&'a I, &'a Ty<I>>,
}

impl<'a, I: DefinitionReference> TypeSubstitution<'a, I> {
    pub fn bind(&mut self, owner: &'a I, position: usize, value: &'a Ty<I>) {
        self.parameters
            .entry(owner)
            .or_default()
            .insert(position, value);
    }

    pub fn bind_receiver(&mut self, owner: &'a I, value: &'a Ty<I>) {
        self.receivers.insert(owner, value);
    }

    pub fn for_owner(owner: &'a I, arguments: &'a [Ty<I>]) -> Self {
        let mut substitution = Self::default();
        for (position, argument) in arguments.iter().enumerate() {
            substitution.bind(owner, position, argument);
        }
        substitution
    }

    pub fn apply(
        &self,
        ty: &Ty<I>,
        cancel: &CancellationToken,
    ) -> Result<Ty<I>, TypeTransformError> {
        Transform::new(self, None, cancel).run(ty)
    }

    pub fn apply_bounds(
        &self,
        bounds: &[GenericBound<I>],
        cancel: &CancellationToken,
    ) -> Result<Vec<GenericBound<I>>, TypeTransformError> {
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
        // Substitution can reorder receivers or make distinct binders equal.
        // Preserve the canonical bound representation after that transformation.
        let mut merged = BTreeMap::<Ty<I>, Vec<Constraint<I>>>::new();
        for bound in applied {
            merged
                .entry(bound.ty)
                .or_default()
                .extend(bound.constraints);
        }
        Ok(merged
            .into_iter()
            .map(|(ty, mut constraints)| {
                constraints.sort();
                constraints.dedup();
                GenericBound { ty, constraints }
            })
            .collect())
    }

    pub fn parameter(&self, owner: &I, position: usize) -> Option<&'a Ty<I>> {
        self.parameters.get(owner)?.get(&position).copied()
    }

    pub fn apply_nominal(
        &self,
        ty: &NominalTy<I>,
        cancel: &CancellationToken,
    ) -> Result<NominalTy<I>, TypeTransformError> {
        let value = Ty::Trait(Transform::new(self, None, cancel).nominal(ty, 1, true)?);
        if !value.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded);
        }
        let Ty::Trait(value) = value else {
            unreachable!("nominal type construction")
        };
        Ok(value)
    }

    fn replacement(&self, ty: &Ty<I>) -> Option<&'a Ty<I>> {
        match ty {
            Ty::Parameter { owner, position } => self.parameters.get(owner)?.get(position).copied(),
            Ty::SelfType(owner) => self.receivers.get(owner).copied(),
            _ => None,
        }
    }
}

impl<I: DefinitionReference> GenericParam<I> {
    pub fn as_type(&self) -> Ty<I> {
        Ty::Parameter {
            owner: self.owner.clone(),
            position: self.position,
        }
    }
}

/// Resolve associated outputs on an applied interface after parameter substitution.
/// Unrelated projections remain symbolic; cyclic or oversized expansions fail.
pub fn resolve_associated_outputs<I: DefinitionReference>(
    ty: &Ty<I>,
    interface: &NominalTy<I>,
    cancel: &CancellationToken,
) -> Result<Ty<I>, TypeTransformError> {
    Transform::new(&TypeSubstitution::default(), Some(interface), cancel).run(ty)
}

/// Resolve projections using a caller's already validated dependency contracts.
/// A missing binding stays symbolic; malformed, ambiguous or unbounded proofs fail.
pub type ProjectionLookup<'a, I = DefinitionPath> =
    dyn Fn(&NominalTy<I>, &Ty<I>, &I, &[Ty<I>]) -> Result<Option<Ty<I>>, TypeTransformError> + 'a;

pub fn normalize_projections<I: DefinitionReference>(
    ty: &Ty<I>,
    lookup: &ProjectionLookup<'_, I>,
    cancel: &CancellationToken,
) -> Result<Ty<I>, TypeTransformError> {
    let substitution = TypeSubstitution::default();
    let mut transform = Transform::new(&substitution, None, cancel);
    transform.lookup = Some(lookup);
    transform.run(ty)
}

type ParameterLookup<'a, I> = dyn Fn(&I, usize) -> Option<&'a Ty<I>> + 'a;

/// Apply the same bounded, once-only substitution using an explicit contextual
/// binder lookup. Runtime frames can retain compact owners without rebuilding
/// owned-path maps for every substitution.
pub fn substitute_parameters<'a, I: DefinitionReference + 'a>(
    ty: &Ty<I>,
    lookup: &ParameterLookup<'a, I>,
    cancel: &CancellationToken,
) -> Result<Ty<I>, TypeTransformError> {
    let substitution = TypeSubstitution::default();
    let mut transform = Transform::new(&substitution, None, cancel);
    transform.parameter_lookup = Some(lookup);
    transform.run(ty)
}

struct Transform<'a, 'b, I> {
    substitution: &'a TypeSubstitution<'b, I>,
    outputs: Option<&'a NominalTy<I>>,
    lookup: Option<&'a ProjectionLookup<'a, I>>,
    parameter_lookup: Option<&'a ParameterLookup<'b, I>>,
    cancel: &'a CancellationToken,
    remaining: usize,
}

impl<'a, 'b, I: DefinitionReference> Transform<'a, 'b, I> {
    fn new(
        substitution: &'a TypeSubstitution<'b, I>,
        outputs: Option<&'a NominalTy<I>>,
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

    fn id(&self, id: &I) -> Result<I, TypeTransformError> {
        if !id.within_path_limit() {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(id.clone())
    }

    fn run(&mut self, ty: &Ty<I>) -> Result<Ty<I>, TypeTransformError> {
        let result = self.visit(ty, 1, true)?;
        if !result.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(result)
    }

    fn many(
        &mut self,
        types: &[Ty<I>],
        depth: usize,
        replace: bool,
    ) -> Result<Vec<Ty<I>>, TypeTransformError> {
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
        ty: &NominalTy<I>,
        depth: usize,
        replace: bool,
    ) -> Result<NominalTy<I>, TypeTransformError> {
        self.step(depth)?;
        if ty.associated_types.len() > MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(NominalTy {
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
        ty: &Ty<I>,
        depth: usize,
        replace: bool,
    ) -> Result<Ty<I>, TypeTransformError> {
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
            Ty::Host(id) => Ty::Host(self.id(id)?),
            Ty::SelfType(id) => Ty::SelfType(self.id(id)?),
            Ty::Parameter { owner, position } => Ty::Parameter {
                owner: self.id(owner)?,
                position: *position,
            },
            Ty::Tuple(types) => Ty::Tuple(self.many(types, depth + 1, replace)?),
            Ty::Function { params, result } => Ty::Function {
                params: self.many(params, depth + 1, replace)?,
                result: Box::new(self.visit(result, depth + 1, replace)?),
            },
            Ty::Iter(ty) => Ty::Iter(Box::new(self.visit(ty, depth + 1, replace)?)),
            Ty::Range(ty, kind) => Ty::Range(Box::new(self.visit(ty, depth + 1, replace)?), *kind),
            Ty::Array(ty, access) => {
                Ty::Array(Box::new(self.visit(ty, depth + 1, replace)?), *access)
            }
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
                        if let Some(outputs) = self.outputs
                            && interface.declaration == outputs.declaration
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
                    member: self.id(member)?,
                    arguments,
                }
            }
        })
    }
}

impl<I> Default for TypeSubstitution<'_, I> {
    fn default() -> Self {
        Self {
            parameters: BTreeMap::new(),
            receivers: BTreeMap::new(),
        }
    }
}
#[cfg(test)]
mod tests;
