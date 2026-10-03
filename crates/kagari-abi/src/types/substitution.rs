//! Bounded binder substitution over executable types, without source inference.
use crate::types::{AbiType, ConstraintAbi, GenericBoundAbi, GenericParameterAbi, NominalAbiType};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, reference::DefinitionReference},
};
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
pub struct TypeSubstitution<'a, I = DefinitionPath> {
    parameters: BTreeMap<&'a I, BTreeMap<usize, &'a AbiType<I>>>,
    receivers: BTreeMap<&'a I, &'a AbiType<I>>,
}

impl<'a, I: DefinitionReference> TypeSubstitution<'a, I> {
    pub fn bind(&mut self, owner: &'a I, position: usize, value: &'a AbiType<I>) {
        self.parameters
            .entry(owner)
            .or_default()
            .insert(position, value);
    }

    pub fn bind_receiver(&mut self, owner: &'a I, value: &'a AbiType<I>) {
        self.receivers.insert(owner, value);
    }

    pub fn for_owner(owner: &'a I, arguments: &'a [AbiType<I>]) -> Self {
        let mut substitution = Self::default();
        for (position, argument) in arguments.iter().enumerate() {
            substitution.bind(owner, position, argument);
        }
        substitution
    }

    pub fn apply(
        &self,
        ty: &AbiType<I>,
        cancel: &CancellationToken,
    ) -> Result<AbiType<I>, TypeTransformError> {
        Transform::new(self, None, cancel).run(ty)
    }

    pub fn apply_bounds(
        &self,
        bounds: &[GenericBoundAbi<I>],
        cancel: &CancellationToken,
    ) -> Result<Vec<GenericBoundAbi<I>>, TypeTransformError> {
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
            .collect::<Result<Vec<_>, TypeTransformError>>()?;
        // Substitution can reorder receivers or make distinct binders equal.
        // Preserve the canonical bound representation after that transformation.
        let mut merged = BTreeMap::<AbiType<I>, Vec<ConstraintAbi<I>>>::new();
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
                GenericBoundAbi { ty, constraints }
            })
            .collect())
    }

    pub fn parameter(&self, owner: &I, position: usize) -> Option<&'a AbiType<I>> {
        self.parameters.get(owner)?.get(&position).copied()
    }

    pub fn apply_nominal(
        &self,
        ty: &NominalAbiType<I>,
        cancel: &CancellationToken,
    ) -> Result<NominalAbiType<I>, TypeTransformError> {
        let value = AbiType::Trait(Transform::new(self, None, cancel).nominal(ty, 1, true)?);
        if !value.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded);
        }
        let AbiType::Trait(value) = value else {
            unreachable!("nominal type construction")
        };
        Ok(value)
    }

    fn replacement(&self, ty: &AbiType<I>) -> Option<&'a AbiType<I>> {
        match ty {
            AbiType::Parameter { owner, position } => {
                self.parameters.get(owner)?.get(position).copied()
            }
            AbiType::SelfType(owner) => self.receivers.get(owner).copied(),
            _ => None,
        }
    }
}

impl<I: DefinitionReference> GenericParameterAbi<I> {
    pub fn as_type(&self) -> AbiType<I> {
        AbiType::Parameter {
            owner: self.owner.clone(),
            position: self.position,
        }
    }
}

/// Resolve associated outputs on an applied interface after parameter substitution.
/// Unrelated projections remain symbolic; cyclic or oversized expansions fail.
pub fn resolve_associated_outputs<I: DefinitionReference>(
    ty: &AbiType<I>,
    interface: &NominalAbiType<I>,
    cancel: &CancellationToken,
) -> Result<AbiType<I>, TypeTransformError> {
    Transform::new(&TypeSubstitution::default(), Some(interface), cancel).run(ty)
}

/// Resolve projections using a caller's already validated dependency contracts.
/// A missing binding stays symbolic; malformed, ambiguous or unbounded proofs fail.
pub type ProjectionLookup<'a, I = DefinitionPath> = dyn Fn(
        &NominalAbiType<I>,
        &AbiType<I>,
        &I,
        &[AbiType<I>],
    ) -> Result<Option<AbiType<I>>, TypeTransformError>
    + 'a;

pub fn normalize_projections<I: DefinitionReference>(
    ty: &AbiType<I>,
    lookup: &ProjectionLookup<'_, I>,
    cancel: &CancellationToken,
) -> Result<AbiType<I>, TypeTransformError> {
    let substitution = TypeSubstitution::default();
    let mut transform = Transform::new(&substitution, None, cancel);
    transform.lookup = Some(lookup);
    transform.run(ty)
}

type ParameterLookup<'a, I> = dyn Fn(&I, usize) -> Option<&'a AbiType<I>> + 'a;

/// Apply the same bounded, once-only substitution using an explicit contextual
/// binder lookup. Runtime frames can retain compact owners without rebuilding
/// owned-path maps for every substitution.
pub fn substitute_parameters<'a, I: DefinitionReference + 'a>(
    ty: &AbiType<I>,
    lookup: &ParameterLookup<'a, I>,
    cancel: &CancellationToken,
) -> Result<AbiType<I>, TypeTransformError> {
    let substitution = TypeSubstitution::default();
    let mut transform = Transform::new(&substitution, None, cancel);
    transform.parameter_lookup = Some(lookup);
    transform.run(ty)
}

struct Transform<'a, 'b, I> {
    substitution: &'a TypeSubstitution<'b, I>,
    outputs: Option<&'a NominalAbiType<I>>,
    lookup: Option<&'a ProjectionLookup<'a, I>>,
    parameter_lookup: Option<&'a ParameterLookup<'b, I>>,
    cancel: &'a CancellationToken,
    remaining: usize,
}

impl<'a, 'b, I: DefinitionReference> Transform<'a, 'b, I> {
    fn new(
        substitution: &'a TypeSubstitution<'b, I>,
        outputs: Option<&'a NominalAbiType<I>>,
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

    fn run(&mut self, ty: &AbiType<I>) -> Result<AbiType<I>, TypeTransformError> {
        let result = self.visit(ty, 1, true)?;
        if !result.within_wire_limits() {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(result)
    }

    fn many(
        &mut self,
        types: &[AbiType<I>],
        depth: usize,
        replace: bool,
    ) -> Result<Vec<AbiType<I>>, TypeTransformError> {
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
        ty: &NominalAbiType<I>,
        depth: usize,
        replace: bool,
    ) -> Result<NominalAbiType<I>, TypeTransformError> {
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
        ty: &AbiType<I>,
        depth: usize,
        replace: bool,
    ) -> Result<AbiType<I>, TypeTransformError> {
        self.step(depth)?;
        if replace {
            let replacement = if let AbiType::Parameter { owner, position } = ty
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
            AbiType::NativeObject(ty) => AbiType::NativeObject(self.nominal(ty, depth, replace)?),
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
