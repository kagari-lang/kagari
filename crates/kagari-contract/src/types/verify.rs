//! Validate serialized semantic types independently of display strings.
use crate::{
    layout::LayoutValidationError,
    types::{InterfaceTable, PublicItem, TraitContract},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity,
    identity::{DefinitionKind, ModuleIdentity},
};
use kagari_types::declaration::verify::{
    Parameters, aggregate_shape_valid, bounds_valid, function_valid, owner, parameters,
    scalar_const_valid, trait_valid, type_valid,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, TraitDef, TypeDefKind, ownership::ReceiverOwners},
    ty::{
        Constraint, GenericParam, NominalTy, Ty,
        matching::{ImplementationPattern, projection_output},
        substitution::{TypeSubstitution, TypeTransformError, normalize_projections},
    },
};
use std::{
    collections::{BTreeMap, HashSet},
    iter,
};

#[cfg(test)]
use kagari_types::collection::CollectionAccess;

pub fn validate(
    items: &[PublicItem],
    module: &ModuleIdentity,
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    let invalid = || LayoutValidationError::Invalid;
    let mut aggregate_names = HashSet::new();
    let mut trait_names = HashSet::new();
    let mut interface_identities = HashSet::new();
    let receivers = ReceiverOwners::from_types(items.iter().filter_map(|item| match item {
        PublicItem::Type(ty) => Some((module, ty)),
        _ => None,
    }))
    .map_err(|_| invalid())?;
    for item in items {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let valid = match item {
            PublicItem::Function(function) => {
                (matches!(function.implementation, CallableImplementation::Native(_))
                    || (function.generic_params.is_empty() && function.bounds.is_empty()))
                    && function_valid(function, module, &[], &Parameters::new(), None, cancel)
            }
            PublicItem::Const(value) => type_valid(&value.ty, &Parameters::new(), None, cancel),
            PublicItem::Type(ty) => {
                let kind = match ty.kind {
                    TypeDefKind::Struct => DefinitionKind::Struct,
                    TypeDefKind::Enum => DefinitionKind::Enum,
                    TypeDefKind::Native(kind) => kind.declaration_kind(),
                    TypeDefKind::NativeStorage(_) => DefinitionKind::AssociatedType,
                };
                let owner = owner(module, &[], kind, &ty.name);
                aggregate_names.insert(&ty.name)
                    && aggregate_shape_valid(ty, cancel)
                    && parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(
                        |params| {
                            bounds_valid(&ty.bounds, &params, cancel)
                                && ty
                                    .fields
                                    .iter()
                                    .all(|field| type_valid(&field.ty, &params, None, cancel))
                                && ty
                                    .variants
                                    .iter()
                                    .flat_map(|variant| &variant.payload)
                                    .all(|ty| type_valid(ty, &params, None, cancel))
                        },
                    )
            }
            PublicItem::Trait(ty) => {
                trait_names.insert(&ty.name) && trait_valid(ty, module, cancel)
            }
            PublicItem::InherentTable(table) => {
                let owner = &table.declaration;
                let params = (interface_identities.insert(owner)
                    && owner.module == *module
                    && owner.path.len() == 1
                    && owner.path[0].kind == DefinitionKind::Impl
                    && owner.path[0].name.is_empty()
                    && owner.within_path_limit())
                .then(|| parameters(&table.generic_params, owner, &Parameters::new()))
                .flatten();
                params.is_some_and(|params| {
                    let mut names = HashSet::new();
                    !table.methods.is_empty()
                        && receivers.owner(&table.for_type).as_ref() == Some(module)
                        && bounds_valid(&table.bounds, &params, cancel)
                        && type_valid(&table.for_type, &params, None, cancel)
                        && table.methods.iter().all(|method| {
                            names.insert(&method.name)
                                && method.params.first().is_none_or(|receiver| {
                                    receiver.name != "self" || receiver.ty == table.for_type
                                })
                                && function_valid(
                                    method,
                                    module,
                                    &owner.path,
                                    &params,
                                    None,
                                    cancel,
                                )
                        })
                })
            }
            PublicItem::InterfaceTable(table) => {
                let owner = &table.declaration;
                let params = (interface_identities.insert(owner)
                    && owner.module == *module
                    && owner.path.len() == 1
                    && owner.path[0].kind == DefinitionKind::Impl
                    && owner.path[0].name.is_empty()
                    && owner.within_path_limit())
                .then(|| parameters(&table.generic_params, owner, &Parameters::new()))
                .flatten();
                params.is_some_and(|params| {
                    let mut methods = HashSet::new();
                    ({
                        let mut names = HashSet::new();
                        table.associated_consts.iter().all(|member| {
                            !member.name.is_empty()
                                && names.insert(&member.name)
                                && scalar_const_valid(&member.ty, &member.value)
                        })
                    }) && bounds_valid(&table.bounds, &params, cancel)
                        && families_valid(table, &params, cancel)
                        && (!table.host_bridge
                            || (table.generic_params.is_empty()
                                && table.bounds.is_empty()
                                && matches!(table.for_type, Ty::Host(_))
                                && table.trait_type.is_concrete()))
                        && matches!(table.trait_type, Ty::Trait(_))
                        && type_valid(&table.trait_type, &params, None, cancel)
                        && type_valid(&table.for_type, &params, None, cancel)
                        && table.methods.iter().all(|method| {
                            methods.insert(&method.name)
                                && function_valid(
                                    method,
                                    module,
                                    &owner.path,
                                    &params,
                                    None,
                                    cancel,
                                )
                        })
                })
            }
        };
        if !valid {
            cancel
                .check()
                .map_err(|_| LayoutValidationError::Cancelled)?;
            return Err(invalid());
        }
    }
    for item in items {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let PublicItem::InterfaceTable(table) = item else {
            continue;
        };
        let Ty::Trait(instance) = &table.trait_type else {
            return Err(invalid());
        };
        if instance.declaration.module != *module {
            continue;
        }
        if instance.declaration.path.len() != 1
            || instance.declaration.path[0].kind != DefinitionKind::Trait
            || instance.declaration.path[0].occurrence != 0
        {
            return Err(invalid());
        }
        let Some(trait_name) = instance.declaration.path.last().map(|part| &part.name) else {
            return Err(invalid());
        };
        let Some(PublicItem::Trait(interface)) = items
            .iter()
            .find(|item| matches!(item, PublicItem::Trait(ty) if &ty.name == trait_name))
        else {
            // Private traits are absent from the public ABI table.
            continue;
        };
        if !interface_contract_matches(table, interface, cancel) {
            cancel
                .check()
                .map_err(|_| LayoutValidationError::Cancelled)?;
            return Err(invalid());
        }
    }
    Ok(())
}

pub fn validate_trait_contracts(
    contracts: &[TraitContract],
    items: &[PublicItem],
    module: &ModuleIdentity,
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    let mut names = HashSet::new();
    for contract in contracts {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let id = &contract.declaration;
        if id.module != *module
            || id.path.len() != 1
            || id.path[0].kind != DefinitionKind::Trait
            || id.path[0].occurrence != 0
            || id.path[0].name != contract.abi.name
            || !id.within_path_limit()
            || !names.insert(&contract.abi.name)
            || !trait_valid(&contract.abi, module, cancel)
        {
            return Err(LayoutValidationError::Invalid);
        }
    }
    if items
        .iter()
        .any(|item| matches!(item, PublicItem::Trait(public) if names.contains(&public.name)))
    {
        return Err(LayoutValidationError::Invalid);
    }
    for item in items {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let PublicItem::InterfaceTable(table) = item else {
            continue;
        };
        let Ty::Trait(instance) = &table.trait_type else {
            return Err(LayoutValidationError::Invalid);
        };
        if instance.declaration.module != *module
            || items.iter().any(|item| {
                matches!(item, PublicItem::Trait(public) if instance.declaration.path.last().is_some_and(|part| part.name == public.name))
            })
        {
            continue;
        }
        let Some(contract) = contracts
            .iter()
            .find(|contract| contract.declaration == instance.declaration)
        else {
            return Err(LayoutValidationError::Invalid);
        };
        if !interface_contract_matches(table, &contract.abi, cancel) {
            cancel
                .check()
                .map_err(|_| LayoutValidationError::Cancelled)?;
            return Err(LayoutValidationError::Invalid);
        }
    }
    Ok(())
}

/// Engine defaults are declared once on their canonical protocol. They need no
/// per-implementation ABI entry, especially when their bounds do not hold here.
fn required_methods_present(table: &InterfaceTable, interface: &TraitDef) -> bool {
    interface.methods.iter().all(|method| {
        table
            .methods
            .iter()
            .any(|actual| actual.name == method.name)
    })
}

pub fn interface_contract_matches(
    table: &InterfaceTable,
    interface: &TraitDef,
    cancel: &CancellationToken,
) -> bool {
    let Ty::Trait(instance) = &table.trait_type else {
        return false;
    };
    let mut matched = HashSet::new();
    interface_constants_match(table, interface)
        && instance.arguments.len() == interface.generic_params.len()
        && interface_families_match(table, interface, cancel)
        && instance.associated_types.len()
            == interface
                .associated_types
                .iter()
                .filter(|member| member.generic_params.is_empty())
                .count()
        && interface
            .associated_types
            .iter()
            .filter(|member| member.generic_params.is_empty())
            .all(|member| instance.associated_types.contains_key(&member.declaration))
        && required_methods_present(table, interface)
        && table.methods.iter().all(|method| {
            cancel.check().is_ok()
                && interface.methods.iter().any(|declared| {
                    declared.name == method.name
                        && same_method_contract(declared, method, instance, table, cancel)
                        && matched.insert(&declared.name)
                })
        })
}

pub fn interface_constants_match(table: &InterfaceTable, interface: &TraitDef) -> bool {
    {
        let mut names = HashSet::new();
        table.associated_consts.iter().all(|member| {
            names.insert(&member.name)
                && interface.associated_consts.iter().any(|declared| {
                    declared
                        .declaration
                        .path
                        .last()
                        .is_some_and(|part| part.name == member.name)
                        && declared.ty == member.ty
                        && scalar_const_valid(&member.ty, &member.value)
                })
        }) && interface.associated_consts.iter().all(|member| {
            member.default_value.is_some()
                || table.associated_consts.iter().any(|actual| {
                    member
                        .declaration
                        .path
                        .last()
                        .is_some_and(|part| part.name == actual.name)
                })
        })
    }
}

fn families_valid(table: &InterfaceTable, outer: &Parameters, cancel: &CancellationToken) -> bool {
    let Ty::Trait(interface) = &table.trait_type else {
        return false;
    };
    let mut seen = HashSet::new();
    table.associated_type_families.iter().all(|family| {
        let name = family
            .declaration
            .path
            .last()
            .map_or("", |part| part.name.as_str());
        !name.is_empty()
            && seen.insert(&family.declaration)
            && family.declaration == identity::associated_type_id(&interface.declaration, name)
            && !family.generic_params.is_empty()
            && parameters(
                &family.generic_params,
                &identity::associated_type_id(&table.declaration, name),
                outer,
            )
            .is_some_and(|params| {
                bounds_valid(&family.bounds, &params, cancel)
                    && type_valid(&family.value, &params, None, cancel)
            })
    })
}

pub fn interface_families_match(
    table: &InterfaceTable,
    interface: &TraitDef,
    cancel: &CancellationToken,
) -> bool {
    let Ty::Trait(instance) = &table.trait_type else {
        return false;
    };
    let families = interface
        .associated_types
        .iter()
        .filter(|member| !member.generic_params.is_empty())
        .collect::<Vec<_>>();
    if families.len() != table.associated_type_families.len() {
        return false;
    }
    families.into_iter().all(|member| {
        let Some(family) = table
            .associated_type_families
            .iter()
            .find(|family| family.declaration == member.declaration)
        else {
            return false;
        };
        if member.generic_params.len() != family.generic_params.len() {
            return false;
        }
        let actual_parameters = family
            .generic_params
            .iter()
            .map(GenericParam::as_type)
            .collect::<Vec<_>>();
        let mut substitution =
            TypeSubstitution::for_owner(&instance.declaration, &instance.arguments);
        for (expected, actual) in member.generic_params.iter().zip(&actual_parameters) {
            substitution.bind(&expected.owner, expected.position, actual);
        }
        substitution.bind_receiver(&instance.declaration, &table.for_type);
        let Ok(expected) = substitution.apply_bounds(&member.parameter_bounds, cancel) else {
            return false;
        };
        cancel.check().is_ok()
            && family.bounds.iter().all(|bound| {
                expected.iter().any(|required| {
                    bound.ty == required.ty
                        && bound
                            .constraints
                            .iter()
                            .all(|constraint| required.constraints.contains(constraint))
                })
            })
    })
}

fn same_method_contract(
    declared: &FnDecl,
    implemented: &FnDecl,
    instance: &NominalTy,
    table: &InterfaceTable,
    cancel: &CancellationToken,
) -> bool {
    if declared.generic_params.len() != implemented.generic_params.len()
        || declared.params.len() != implemented.params.len()
    {
        return false;
    }
    let normalize = |ty: &Ty| {
        normalize_projections(
            ty,
            &|interface, receiver, member, arguments| {
                let Ty::Trait(implemented) = &table.trait_type else {
                    return Err(TypeTransformError::InvalidContract);
                };
                projection_output(
                    ImplementationPattern {
                        storage_access: None,
                        parameters: &table.generic_params,
                        receiver: &table.for_type,
                        interface: implemented,
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
    method_contract_matches(
        declared,
        implemented,
        instance,
        table,
        &normalize,
        true,
        cancel,
    )
}

pub fn interface_methods_match(
    table: &InterfaceTable,
    interface: &TraitDef,
    normalize: &dyn Fn(&Ty) -> Result<Ty, TypeTransformError>,
    cancel: &CancellationToken,
) -> bool {
    let Ty::Trait(instance) = &table.trait_type else {
        return false;
    };
    required_methods_present(table, interface)
        && table.methods.iter().all(|implemented| {
            interface
                .methods
                .iter()
                .find(|declared| declared.name == implemented.name)
                .is_some_and(|declared| {
                    method_contract_matches(
                        declared,
                        implemented,
                        instance,
                        table,
                        normalize,
                        false,
                        cancel,
                    )
                })
        })
}

fn method_contract_matches(
    declared: &FnDecl,
    implemented: &FnDecl,
    instance: &NominalTy,
    table: &InterfaceTable,
    normalize: &dyn Fn(&Ty) -> Result<Ty, TypeTransformError>,
    defer_projection: bool,
    cancel: &CancellationToken,
) -> bool {
    if matches!(implemented.implementation, CallableImplementation::Required)
        || declared.generic_params.len() != implemented.generic_params.len()
        || declared.params.len() != implemented.params.len()
    {
        return false;
    }
    let actual_parameters = implemented
        .generic_params
        .iter()
        .map(GenericParam::as_type)
        .collect::<Vec<_>>();
    let mut parameters = TypeSubstitution::for_owner(&instance.declaration, &instance.arguments);
    for (expected, actual) in declared.generic_params.iter().zip(&actual_parameters) {
        parameters.bind(&expected.owner, expected.position, actual);
    }
    let mut receiver = TypeSubstitution::default();
    receiver.bind_receiver(&instance.declaration, &table.for_type);
    let expected = |ty: &Ty| {
        let ty = receiver.apply(ty, cancel)?;
        let ty = parameters.apply(&ty, cancel)?;
        normalize(&ty)
    };
    let actual = |ty: &Ty| normalize(ty);
    let inherited_default = match (&declared.implementation, &implemented.implementation) {
        (
            CallableImplementation::NativeDefault(declared),
            CallableImplementation::NativeDefault(implemented),
        ) => {
            declared.declaration == implemented.declaration
                && declared.arguments.len() == implemented.arguments.len()
                && declared.arguments.iter().zip(&implemented.arguments).all(
                    |(declared, implemented)| {
                        expected(declared).is_ok_and(|declared| {
                            actual(implemented).is_ok_and(|implemented| {
                                declared == implemented
                                    || defer_projection
                                        && (declared.contains_projection()
                                            || implemented.contains_projection())
                            })
                        })
                    },
                )
        }
        (_, CallableImplementation::NativeDefault(_)) => false,
        _ => declared.method_policy.override_allowed,
    };
    if !inherited_default {
        return false;
    }
    let signatures = iter::once(expected(&declared.return_type))
        .chain(iter::once(actual(&implemented.return_type)))
        .chain(declared.params.iter().map(|p| expected(&p.ty)))
        .chain(implemented.params.iter().map(|p| actual(&p.ty)))
        .collect::<Result<Vec<_>, _>>();
    let Ok(signatures) = signatures else {
        return false;
    };
    // Dependency projections are rechecked with the complete linked resolver.
    if defer_projection && signatures.iter().any(Ty::contains_projection) {
        return true;
    }
    let bounds = |function: &FnDecl, normalize: &dyn Fn(&Ty) -> Result<Ty, TypeTransformError>| {
        function
            .bounds
            .iter()
            .map(|bound| {
                let mut constraints = bound
                    .constraints
                    .iter()
                    .map(|constraint| {
                        Some(match constraint {
                            Constraint::Standard(value) => Constraint::Standard(*value),
                            Constraint::Trait(value) => {
                                let Ty::Trait(value) = normalize(&Ty::Trait(value.clone())).ok()?
                                else {
                                    return None;
                                };
                                Constraint::Trait(value)
                            }
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                constraints.sort();
                Some((normalize(&bound.ty).ok()?, constraints))
            })
            .collect::<Option<BTreeMap<_, _>>>()
    };
    cancel.check().is_ok()
        && bounds(declared, &expected)
            .is_some_and(|declared| Some(declared) == bounds(implemented, &actual))
        && signatures[0] == signatures[1]
        && declared
            .params
            .iter()
            .zip(&implemented.params)
            .enumerate()
            .all(|(index, (left, right))| {
                cancel.check().is_ok()
                    && left.mutable == right.mutable
                    && signatures[2 + index] == signatures[2 + declared.params.len() + index]
            })
}

#[cfg(test)]
mod tests;
