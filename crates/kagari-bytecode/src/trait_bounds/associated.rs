use crate::{module::BytecodeModule, trait_bounds::contract};
use kagari_abi::types::{
    AbiType, ConstraintAbi, GenericBoundAbi, GenericParameterAbi, InterfaceTableAbi,
    NominalAbiType, TraitAbi,
    applications::ApplicationValidator,
    proofs::{ProofCatalog, host_application},
    substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
};
use kagari_common::cancellation::CancellationToken;

/// Discharge family parameter bounds after validating the carried applications.
fn projection_bounds_valid(
    ty: &AbiType,
    assumptions: &[GenericBoundAbi],
    catalog: &ProofCatalog<'_>,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    ApplicationValidator::new(cancel, |id| contract(id, closure)).validate_type(ty)?;
    let mut pending = vec![ty];
    let mut remaining = 8192usize;
    while let Some(ty) = pending.pop() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if remaining == 0 {
            return Err(TypeTransformError::LimitExceeded);
        }
        remaining -= 1;
        match ty {
            AbiType::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                let Some(record) = contract(&interface.declaration, closure) else {
                    return Ok(false);
                };
                let Some(definition) = record
                    .associated_types
                    .iter()
                    .find(|definition| &definition.declaration == member)
                else {
                    return Ok(false);
                };
                let mut substitution = TypeSubstitution::default();
                for (parameter, actual) in record
                    .generic_params
                    .iter()
                    .zip(&interface.arguments)
                    .chain(definition.generic_params.iter().zip(arguments))
                {
                    substitution.bind(&parameter.owner, parameter.position, actual);
                }
                substitution.bind_receiver(&interface.declaration, receiver);
                for bound in substitution.apply_bounds(&definition.parameter_bounds, cancel)? {
                    let actual = catalog.normalize(&bound.ty, cancel)?;
                    if !catalog.constraints_hold(
                        &actual,
                        &bound.constraints,
                        assumptions,
                        cancel,
                    )? {
                        return Ok(false);
                    }
                }
                pending.push(receiver);
                pending.extend(&interface.arguments);
                pending.extend(interface.associated_types.values());
                pending.extend(arguments);
            }
            AbiType::Struct(n) | AbiType::Enum(n) | AbiType::Trait(n) => {
                pending.extend(&n.arguments);
                pending.extend(n.associated_types.values());
            }
            AbiType::Tuple(items) | AbiType::StandardEnum { args: items, .. } => {
                pending.extend(items)
            }
            AbiType::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            AbiType::Array(item, _)
            | AbiType::Set(item, _)
            | AbiType::Iter(item)
            | AbiType::Range(item, _) => pending.push(item),
            AbiType::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            _ => {}
        }
    }
    Ok(true)
}

pub(super) fn associated_bounds_match(
    table: &InterfaceTableAbi,
    interface: &NominalAbiType,
    record: &TraitAbi,
    catalog: &ProofCatalog<'_>,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for member in &record.associated_types {
        let mut substitution = TypeSubstitution::default();
        for (parameter, actual) in record.generic_params.iter().zip(&interface.arguments) {
            substitution.bind(&parameter.owner, parameter.position, actual);
        }
        let mut available = table.bounds.clone();
        let family_inputs;
        let actual = if member.generic_params.is_empty() {
            let Some(actual) = interface.associated_types.get(&member.declaration) else {
                return Ok(false);
            };
            actual.clone()
        } else {
            let Some(family) = table
                .associated_type_families
                .iter()
                .find(|family| family.declaration == member.declaration)
            else {
                return Ok(false);
            };
            family_inputs = family
                .generic_params
                .iter()
                .map(GenericParameterAbi::as_type)
                .collect::<Vec<_>>();
            for (parameter, actual) in member.generic_params.iter().zip(&family_inputs) {
                substitution.bind(&parameter.owner, parameter.position, actual);
            }
            available.extend(family.bounds.clone());
            available.extend(substitution.apply_bounds(&member.parameter_bounds, cancel)?);
            available = catalog.expand_bounds(&available, cancel)?;
            if !projection_bounds_valid(&family.value, &available, catalog, closure, cancel)? {
                return Ok(false);
            }
            catalog.normalize(&family.value, cancel)?
        };
        substitution.bind_receiver(&interface.declaration, &table.for_type);
        let constraints = member
            .bounds
            .iter()
            .map(|constraint| {
                Ok(match constraint {
                    ConstraintAbi::Standard(value) => ConstraintAbi::Standard(*value),
                    ConstraintAbi::Trait(required) => {
                        let ty = substitution.apply(&AbiType::Trait(required.clone()), cancel)?;
                        let AbiType::Trait(required) =
                            resolve_associated_outputs(&ty, interface, cancel)?
                        else {
                            return Err(TypeTransformError::InvalidContract);
                        };
                        ConstraintAbi::Trait(required)
                    }
                })
            })
            .collect::<Result<Vec<_>, TypeTransformError>>()?;
        if !catalog.constraints_hold(&actual, &constraints, &available, cancel)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn host_bounds_match(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for host in &module.host_interface.types {
        for implementation in &host.trait_implementations {
            if !closure
                .iter()
                .any(|member| member.identity == implementation.trait_id.module)
            {
                continue;
            }
            let Some(record) = contract(&implementation.trait_id, closure) else {
                return Ok(false);
            };
            let applied = host_application(implementation);
            let receiver = AbiType::Host(host.id.clone());
            let mut substitution = TypeSubstitution::default();
            substitution.bind_receiver(&applied.declaration, &receiver);
            for (parameter, actual) in record.generic_params.iter().zip(&applied.arguments) {
                substitution.bind(&parameter.owner, parameter.position, actual);
            }
            let mut obligations = record.bounds.clone();
            for member in &record.associated_types {
                let Some(actual) = applied.associated_types.get(&member.declaration) else {
                    return Ok(false);
                };
                obligations.push(GenericBoundAbi {
                    ty: actual.clone(),
                    constraints: member.bounds.clone(),
                });
            }
            let normalize = |ty: &AbiType| {
                let ty = resolve_associated_outputs(ty, &applied, cancel)?;
                catalog.normalize(&substitution.apply(&ty, cancel)?, cancel)
            };
            for bound in obligations {
                let actual = normalize(&bound.ty)?;
                for constraint in bound.constraints {
                    let proven = match constraint {
                        ConstraintAbi::Standard(required) => catalog.constraints_hold(
                            &actual,
                            &[ConstraintAbi::Standard(required)],
                            &[],
                            cancel,
                        )?,
                        ConstraintAbi::Trait(required) => {
                            let AbiType::Trait(required) = normalize(&AbiType::Trait(required))?
                            else {
                                return Ok(false);
                            };
                            catalog.implementation_count(&required, &actual, &[], cancel)? == 1
                        }
                    };
                    if !proven {
                        return Ok(false);
                    }
                }
            }
        }
    }
    Ok(true)
}
