//! Ordinary inherited method materialization from a canonical declaration and applied impl.

use crate::{
    types::{ConcreteFunctionIdentity, InterfaceTable, named::FunctionOrigin},
    updates::SourceRecord,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{NativeDeclaration, Param, requirement::NativeCallableRequirement},
    ty::{
        GenericBound, GenericParam, Ty,
        matching::{ImplementationPattern, projection_output},
        substitution::{
            TypeSubstitution, TypeTransformError, normalize_projections, resolve_associated_outputs,
        },
    },
};
use std::collections::HashMap;

/// Apply the impl's trait, receiver and method binders to an ordinary Native declaration.
/// No Rust binding selection or executable body is invented by this transformation.
pub fn native(
    source: &NativeDeclaration,
    table: &InterfaceTable,
    origin: &FunctionOrigin,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<NativeDeclaration, TypeTransformError> {
    let invalid = TypeTransformError::InvalidContract;
    let Ty::Trait(interface) = &table.trait_type else {
        return Err(invalid);
    };
    if source.path_operation.is_some()
        || source.declaration != origin.authored
        || definitions.parent(origin.authored).ok() != Some(Some(interface.declaration))
        || definitions.parent(origin.placement).ok() != Some(Some(table.declaration))
        || !matches!(
            source.function.implementation,
            CallableImplementation::Native(_)
        )
    {
        return Err(invalid);
    }
    let name = definitions
        .resolve(origin.placement)
        .map_err(|_| invalid)?
        .last()
        .ok_or(invalid)?;
    if name.kind != DefinitionKind::Method
        || name.name != source.function.name
        || name.occurrence != 0
    {
        return Err(invalid);
    }
    let own = source
        .function
        .generic_params
        .iter()
        .filter(|parameter| parameter.owner == source.declaration)
        .map(|parameter| GenericParam {
            owner: origin.placement,
            position: parameter.position,
        })
        .collect::<Vec<_>>();
    let arguments = own.iter().map(GenericParam::as_type).collect::<Vec<_>>();
    let mut substitution =
        TypeSubstitution::for_owner(&interface.declaration, &interface.arguments);
    substitution.bind_receiver(&interface.declaration, &table.for_type);
    for (parameter, argument) in own.iter().zip(&arguments) {
        substitution.bind(&source.declaration, parameter.position, argument);
    }
    let apply = |ty: &Ty| {
        let applied = resolve_associated_outputs(
            &substitution.apply(ty, cancel)?,
            interface,
            &table.for_type,
            cancel,
        )?;
        normalize_projections(
            &applied,
            &|interface, receiver, member, arguments| {
                projection_output(
                    ImplementationPattern {
                        parameters: &table.generic_params,
                        receiver: &table.for_type,
                        interface: match &table.trait_type {
                            Ty::Trait(interface) => interface,
                            _ => unreachable!(),
                        },
                    },
                    &table.associated_type_families,
                    interface,
                    receiver,
                    member,
                    arguments,
                    cancel,
                )
            },
            cancel,
        )
    };
    let mut function = source.function.clone();
    function.generic_params = table.generic_params.iter().cloned().chain(own).collect();
    function.params = function
        .params
        .iter()
        .map(|parameter| {
            Ok(Param {
                name: parameter.name.clone(),
                mutable: parameter.mutable,
                ty: apply(&parameter.ty)?,
            })
        })
        .collect::<Result<_, TypeTransformError>>()?;
    function.return_type = apply(&function.return_type)?;
    let bounds = source
        .function
        .bounds
        .iter()
        .map(|bound| {
            Ok(GenericBound {
                ty: apply(&bound.ty)?,
                constraints: bound
                    .constraints
                    .iter()
                    .map(|constraint| {
                        let Ty::Trait(value) = apply(&Ty::Trait(constraint.clone()))? else {
                            return Err(invalid);
                        };
                        Ok(value)
                    })
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, TypeTransformError>>()?;
    function.bounds = GenericBound::normalize(
        table.bounds.iter().cloned().chain(bounds),
        definitions,
        cancel,
    )?;
    let requirements = source
        .callable_requirements
        .iter()
        .map(|required| {
            let Ty::Trait(interface) = apply(&Ty::Trait(required.interface.clone()))? else {
                return Err(invalid);
            };
            Ok(NativeCallableRequirement {
                receiver: apply(&required.receiver)?,
                interface,
                member: required.member,
                arguments: required
                    .arguments
                    .iter()
                    .map(&apply)
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect::<Result<_, TypeTransformError>>()?;
    Ok(NativeDeclaration {
        path_operation: None,
        declaration: origin.placement,
        function,
        concrete_result: source.concrete_result.as_ref().map(apply).transpose()?,
        callable_requirements: requirements,
    })
}

/// Check internal source/body association for ordinary placements. This is not an
/// independent semantic oracle: installed library bodies authenticate inherited Script
/// implementations at execution linking, outside the untrusted product.
pub fn script_bodies<'a>(
    tables: impl IntoIterator<Item = &'a InterfaceTable>,
    functions: impl IntoIterator<Item = &'a ConcreteFunctionIdentity>,
    sources: &[SourceRecord],
    cancel: &CancellationToken,
) -> Result<(), TypeTransformError> {
    let mut authored = HashMap::new();
    for source in sources {
        for body in &source.bodies {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if authored
                .insert(body, source.declaration)
                .is_some_and(|previous| previous != source.declaration)
            {
                return Err(TypeTransformError::InvalidContract);
            }
        }
    }
    let mut placed = HashMap::<_, Vec<_>>::new();
    for identity in functions {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        placed
            .entry(identity.declaration)
            .or_default()
            .push(identity);
    }
    for table in tables {
        for origin in &table.method_origins {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            for identity in placed.get(&origin.placement).into_iter().flatten() {
                if authored.get(identity) != Some(&origin.authored) {
                    return Err(TypeTransformError::InvalidContract);
                }
            }
        }
    }
    Ok(())
}
