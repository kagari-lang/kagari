//! Check trait applications against declarations in the linked dependency closure.
//! Local validation checks binder ownership and wire shape; this pass checks
//! referenced arity and members without an implicit standard-library catalog.

use crate::{
    callable::CallableImplementation,
    declaration::{FnDecl, TraitDef, TypeDefKind},
    ty::{
        Constraint, GenericBound, NominalTy, Ty,
        substitution::{MAX_TYPE_NODES, TypeTransformError},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionPath};

pub struct ApplicationValidator<'a, F, G> {
    lookup: F,
    nominal: G,
    cancel: &'a CancellationToken,
}

impl<'a, 'declaration, F, G> ApplicationValidator<'a, F, G>
where
    F: Fn(&DefinitionPath) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionPath) -> Option<(TypeDefKind, usize)>,
{
    pub fn new(cancel: &'a CancellationToken, lookup: F, nominal: G) -> Self {
        Self {
            lookup,
            nominal,
            cancel,
        }
    }

    pub fn trait_application_contract(
        &self,
        applied: &NominalTy,
    ) -> Result<&'declaration TraitDef, TypeTransformError> {
        self.cancel
            .check()
            .map_err(|_| TypeTransformError::Cancelled)?;
        let record =
            (self.lookup)(&applied.declaration).ok_or(TypeTransformError::InvalidContract)?;
        if record.generic_params.len() != applied.arguments.len()
            || applied.associated_types.keys().any(|id| {
                !record
                    .associated_types
                    .iter()
                    .any(|member| member.declaration == *id && member.generic_params.is_empty())
            })
        {
            return Err(TypeTransformError::InvalidContract);
        }
        Ok(record)
    }

    pub fn validate_type(&self, ty: &Ty) -> Result<(), TypeTransformError> {
        let mut pending = vec![ty];
        let mut remaining = MAX_TYPE_NODES;
        while let Some(ty) = pending.pop() {
            self.cancel
                .check()
                .map_err(|_| TypeTransformError::Cancelled)?;
            if remaining == 0 {
                return Err(TypeTransformError::LimitExceeded);
            }
            remaining -= 1;
            match ty {
                Ty::Projection {
                    receiver,
                    interface,
                    member,
                    arguments,
                } => {
                    let record = self.trait_application_contract(interface)?;
                    if !record.associated_types.iter().any(|definition| {
                        definition.declaration == *member
                            && definition.generic_params.len() == arguments.len()
                    }) {
                        return Err(TypeTransformError::InvalidContract);
                    }
                    pending.push(receiver);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                    pending.extend(arguments);
                }
                Ty::Trait(applied) => {
                    self.trait_application_contract(applied)?;
                    pending.extend(&applied.arguments);
                    pending.extend(applied.associated_types.values());
                }
                Ty::NativeObject(applied) => {
                    let (kind, arity) = (self.nominal)(&applied.declaration)
                        .ok_or(TypeTransformError::InvalidContract)?;
                    let TypeDefKind::NativeStorage(layout) = kind else {
                        return Err(TypeTransformError::InvalidContract);
                    };
                    if arity != applied.arguments.len()
                        || !applied.associated_types.is_empty()
                        || !layout.valid_parameters(applied.arguments.len())
                    {
                        return Err(TypeTransformError::InvalidContract);
                    }
                    pending.extend(&applied.arguments);
                }
                Ty::Struct(applied) | Ty::Enum(applied) => {
                    let (kind, arity) = (self.nominal)(&applied.declaration)
                        .ok_or(TypeTransformError::InvalidContract)?;
                    let expected = if matches!(ty, Ty::Enum(_)) {
                        TypeDefKind::Enum
                    } else {
                        TypeDefKind::Struct
                    };
                    if kind != expected
                        || arity != applied.arguments.len()
                        || !applied.associated_types.is_empty()
                    {
                        return Err(TypeTransformError::InvalidContract);
                    }
                    pending.extend(&applied.arguments);
                    pending.extend(applied.associated_types.values());
                }
                Ty::Tuple(items) => pending.extend(items),
                Ty::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Ty::Array(item) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _) => {
                    pending.push(item)
                }
                Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Ty::Builtin(_) | Ty::Host(_) | Ty::Parameter { .. } | Ty::SelfType(_) => {}
            }
            if pending.len() > remaining {
                return Err(TypeTransformError::LimitExceeded);
            }
        }
        Ok(())
    }

    /// The nominal itself may be an aggregate; callers validate its kind locally.
    pub fn nominal_arguments(&self, ty: &NominalTy) -> Result<(), TypeTransformError> {
        self.types(ty.arguments.iter().chain(ty.associated_types.values()))
    }

    pub fn trait_application(&self, ty: &NominalTy) -> Result<(), TypeTransformError> {
        self.trait_application_contract(ty)?;
        self.nominal_arguments(ty)
    }

    pub fn types<'ty>(
        &self,
        types: impl IntoIterator<Item = &'ty Ty>,
    ) -> Result<(), TypeTransformError> {
        for ty in types {
            self.validate_type(ty)?;
        }
        Ok(())
    }

    fn constraints(&self, constraints: &[Constraint]) -> Result<(), TypeTransformError> {
        for constraint in constraints {
            if let Constraint::Trait(applied) = constraint {
                self.trait_application(applied)?;
            }
        }
        Ok(())
    }

    pub fn bounds(&self, bounds: &[GenericBound]) -> Result<(), TypeTransformError> {
        for bound in bounds {
            self.validate_type(&bound.ty)?;
            self.constraints(&bound.constraints)?;
        }
        Ok(())
    }

    pub fn function(&self, function: &FnDecl) -> Result<(), TypeTransformError> {
        if let CallableImplementation::NativeDefault(application) = &function.implementation {
            self.types(&application.arguments)?;
        }
        self.bounds(&function.bounds)?;
        self.types(function.params.iter().map(|p| &p.ty))?;
        self.validate_type(&function.return_type)
    }

    pub fn trait_definition(&self, record: &TraitDef) -> Result<(), TypeTransformError> {
        self.bounds(&record.bounds)?;
        for parent in &record.supertraits {
            self.trait_application(parent)?;
        }
        for method in &record.methods {
            self.function(method)?;
        }
        self.types(record.associated_consts.iter().map(|member| &member.ty))?;
        for member in &record.associated_types {
            self.bounds(&member.parameter_bounds)?;
            self.constraints(&member.bounds)?;
        }
        Ok(())
    }
}
