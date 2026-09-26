//! Validate serialized semantic types independently of display strings.
use super::*;
use crate::module::layout::LayoutValidationError;
#[cfg(test)]
use kagari_common::collection::CollectionAccess;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity},
};
use std::collections::HashSet;

type Parameters = HashSet<(DefinitionId, usize)>;

fn scalar_const_type(ty: &AbiType) -> bool {
    matches!(
        ty,
        AbiType::Builtin(
            BuiltinType::Unit | BuiltinType::Bool | BuiltinType::I32 | BuiltinType::F32
        )
    )
}

fn scalar_const_valid(ty: &AbiType, value: &str) -> bool {
    match ty {
        AbiType::Builtin(BuiltinType::Unit) => value == "const-v1:unit",
        AbiType::Builtin(BuiltinType::Bool) => {
            matches!(value, "const-v1:bool:0" | "const-v1:bool:1")
        }
        AbiType::Builtin(BuiltinType::I32) => value
            .strip_prefix("const-v1:i32:")
            .and_then(|value| value.parse::<i32>().ok().map(|n| n.to_string() == value))
            .unwrap_or(false),
        AbiType::Builtin(BuiltinType::F32) => value
            .strip_prefix("const-v1:f32:")
            .and_then(|value| {
                u32::from_str_radix(value, 16)
                    .ok()
                    .map(|bits| format!("{bits:08x}") == value)
            })
            .unwrap_or(false),
        _ => false,
    }
}

pub(crate) fn concrete_type_valid(ty: &AbiType, cancel: &CancellationToken) -> bool {
    type_valid(ty, &Parameters::new(), None, cancel)
}

pub(crate) fn validate(
    items: &[PublicAbiItem],
    module: &ModuleIdentity,
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    let invalid = || LayoutValidationError::Invalid;
    let mut aggregate_names = HashSet::new();
    let mut trait_names = HashSet::new();
    let mut interface_identities = HashSet::new();
    for item in items {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let valid = match item {
            PublicAbiItem::Function(function) => {
                function.generic_params.is_empty()
                    && function.bounds.is_empty()
                    && function_valid(function, module, &[], &Parameters::new(), None, cancel)
            }
            PublicAbiItem::Const(value) => type_valid(&value.ty, &Parameters::new(), None, cancel),
            PublicAbiItem::Type(ty) => {
                let kind = match ty.kind {
                    TypeAbiKind::Struct => DefinitionKind::Struct,
                    TypeAbiKind::Enum => DefinitionKind::Enum,
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
            PublicAbiItem::Trait(ty) => {
                trait_names.insert(&ty.name) && trait_valid(ty, module, cancel)
            }
            PublicAbiItem::InterfaceTable(table) => {
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
                                && matches!(table.for_type, AbiType::Host(_))
                                && table.trait_type.is_concrete()))
                        && matches!(table.trait_type, AbiType::Trait(_))
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
        let PublicAbiItem::InterfaceTable(table) = item else {
            continue;
        };
        let AbiType::Trait(instance) = &table.trait_type else {
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
        let Some(PublicAbiItem::Trait(interface)) = items
            .iter()
            .find(|item| matches!(item, PublicAbiItem::Trait(ty) if &ty.name == trait_name))
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

pub(crate) fn validate_trait_contracts(
    contracts: &[TraitContract],
    items: &[PublicAbiItem],
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
        .any(|item| matches!(item, PublicAbiItem::Trait(public) if names.contains(&public.name)))
    {
        return Err(LayoutValidationError::Invalid);
    }
    for item in items {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let PublicAbiItem::InterfaceTable(table) = item else {
            continue;
        };
        let AbiType::Trait(instance) = &table.trait_type else {
            return Err(LayoutValidationError::Invalid);
        };
        if instance.declaration.module != *module
            || items.iter().any(|item| {
                matches!(item, PublicAbiItem::Trait(public) if instance.declaration.path.last().is_some_and(|part| part.name == public.name))
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

fn trait_valid(ty: &TraitAbi, module: &ModuleIdentity, cancel: &CancellationToken) -> bool {
    let owner = owner(module, &[], DefinitionKind::Trait, &ty.name);
    let mut methods = HashSet::new();
    !ty.name.is_empty()
        && {
            let mut members = HashSet::new();
            ty.associated_consts.iter().all(|member| {
                let name = member
                    .declaration
                    .path
                    .last()
                    .map_or("", |part| part.name.as_str());
                !name.is_empty()
                    && members.insert(&member.declaration)
                    && member.declaration == kagari_hir::types::associated_const_id(&owner, name)
                    && scalar_const_type(&member.ty)
                    && member
                        .default_value
                        .as_ref()
                        .is_none_or(|value| scalar_const_valid(&member.ty, value))
            })
        }
        && {
            let mut defaults = HashSet::new();
            ty.default_methods
                .iter()
                .all(|slot| *slot < ty.methods.len() && defaults.insert(slot))
        }
        && {
            let mut members = HashSet::new();
            ty.associated_types.iter().all(|member| {
                members.insert(&member.declaration)
                    && member.declaration
                        == kagari_hir::types::associated_type_id(
                            &owner,
                            member
                                .declaration
                                .path
                                .last()
                                .map_or("", |part| part.name.as_str()),
                        )
                    && member
                        .declaration
                        .path
                        .last()
                        .is_some_and(|part| !part.name.is_empty())
            })
        }
        && parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(|params| {
            bounds_valid(&ty.bounds, &params, cancel)
                && ty.supertraits.iter().all(|parent| {
                    type_valid(
                        &AbiType::Trait(parent.clone()),
                        &params,
                        Some(&owner),
                        cancel,
                    )
                })
                && ty.associated_types.iter().all(|member| {
                    parameters(&member.generic_params, &member.declaration, &params).is_some_and(
                        |params| {
                            bounds_valid_in(&member.parameter_bounds, &params, Some(&owner), cancel)
                                && constraints_valid(&member.bounds, &params, Some(&owner), cancel)
                        },
                    )
                })
                && ty.methods.iter().all(|method| {
                    methods.insert(&method.name)
                        && function_valid(
                            method,
                            module,
                            &owner.path,
                            &params,
                            Some(&owner),
                            cancel,
                        )
                })
        })
}

pub(crate) fn interface_contract_matches(
    table: &InterfaceTableAbi,
    interface: &TraitAbi,
    cancel: &CancellationToken,
) -> bool {
    let AbiType::Trait(instance) = &table.trait_type else {
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
        && table.methods.len() == interface.methods.len()
        && table.methods.iter().all(|method| {
            cancel.check().is_ok()
                && interface.methods.iter().any(|declared| {
                    declared.name == method.name
                        && same_method_contract(declared, method, instance, table, cancel)
                        && matched.insert(&declared.name)
                })
        })
}

pub(crate) fn interface_constants_match(table: &InterfaceTableAbi, interface: &TraitAbi) -> bool {
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

fn families_valid(
    table: &InterfaceTableAbi,
    outer: &Parameters,
    cancel: &CancellationToken,
) -> bool {
    let AbiType::Trait(interface) = &table.trait_type else {
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
            && family.declaration
                == kagari_hir::types::associated_type_id(&interface.declaration, name)
            && !family.generic_params.is_empty()
            && parameters(
                &family.generic_params,
                &kagari_hir::types::associated_type_id(&table.declaration, name),
                outer,
            )
            .is_some_and(|params| {
                bounds_valid(&family.bounds, &params, cancel)
                    && type_valid(&family.value, &params, None, cancel)
            })
    })
}

pub(crate) fn interface_families_match(
    table: &InterfaceTableAbi,
    interface: &TraitAbi,
    cancel: &CancellationToken,
) -> bool {
    let AbiType::Trait(instance) = &table.trait_type else {
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
        let mut substitution: kagari_hir::types::TypeSubstitution = instance
            .arguments
            .iter()
            .enumerate()
            .map(|(position, value)| {
                (
                    kagari_hir::types::GenericParameterType {
                        owner: instance.declaration.clone(),
                        position,
                        name: String::new(),
                    },
                    value.to_checked_type(),
                )
            })
            .chain(
                member
                    .generic_params
                    .iter()
                    .zip(&family.generic_params)
                    .map(|(expected, actual)| {
                        (
                            kagari_hir::types::GenericParameterType {
                                owner: expected.owner.clone(),
                                position: expected.position,
                                name: String::new(),
                            },
                            kagari_hir::types::TypeId::Generic(
                                kagari_hir::types::GenericParameterType {
                                    owner: actual.owner.clone(),
                                    position: actual.position,
                                    name: String::new(),
                                },
                            ),
                        )
                    }),
            )
            .collect();
        substitution.insert_receiver(
            instance.declaration.clone(),
            table.for_type.to_checked_type(),
        );
        let expected = member
            .parameter_bounds
            .iter()
            .map(|bound| GenericBoundAbi {
                ty: AbiType::from_checked_type(
                    &bound.ty.to_checked_type().instantiate(&substitution),
                ),
                constraints: bound
                    .constraints
                    .iter()
                    .map(|constraint| match constraint {
                        ConstraintAbi::Standard(value) => ConstraintAbi::Standard(*value),
                        ConstraintAbi::Trait(value) => {
                            let AbiType::Trait(value) = AbiType::from_checked_type(
                                &AbiType::Trait(value.clone())
                                    .to_checked_type()
                                    .instantiate(&substitution),
                            ) else {
                                unreachable!("trait bound")
                            };
                            ConstraintAbi::Trait(value)
                        }
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
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
    declared: &FunctionAbi,
    implemented: &FunctionAbi,
    instance: &NominalAbiType,
    table: &InterfaceTableAbi,
    cancel: &CancellationToken,
) -> bool {
    if declared.generic_params.len() != implemented.generic_params.len()
        || declared.params.len() != implemented.params.len()
    {
        return false;
    }
    let Some(signature) = table.checked_signature() else {
        return false;
    };
    let Some(catalog) =
        kagari_hir::aggregates::AggregateCatalog::from_implementation_signatures([signature])
    else {
        return false;
    };
    method_contract_matches(
        declared,
        implemented,
        instance,
        table,
        &catalog,
        true,
        cancel,
    )
}

pub(crate) fn interface_methods_match(
    table: &InterfaceTableAbi,
    interface: &TraitAbi,
    catalog: &kagari_hir::aggregates::AggregateCatalog,
    cancel: &CancellationToken,
) -> bool {
    let AbiType::Trait(instance) = &table.trait_type else {
        return false;
    };
    table.methods.len() == interface.methods.len()
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
                        catalog,
                        false,
                        cancel,
                    )
                })
        })
}

fn method_contract_matches(
    declared: &FunctionAbi,
    implemented: &FunctionAbi,
    instance: &NominalAbiType,
    table: &InterfaceTableAbi,
    catalog: &kagari_hir::aggregates::AggregateCatalog,
    defer_projection: bool,
    cancel: &CancellationToken,
) -> bool {
    if declared.generic_params.len() != implemented.generic_params.len()
        || declared.params.len() != implemented.params.len()
    {
        return false;
    }
    let mut substitution: kagari_hir::types::TypeSubstitution = instance
        .arguments
        .iter()
        .enumerate()
        .map(|(position, argument)| {
            (
                kagari_hir::types::GenericParameterType {
                    owner: instance.declaration.clone(),
                    position,
                    name: String::new(),
                },
                argument.to_checked_type(),
            )
        })
        .collect();
    substitution.extend(
        declared
            .generic_params
            .iter()
            .zip(&implemented.generic_params)
            .map(|(expected, actual)| {
                (
                    kagari_hir::types::GenericParameterType {
                        owner: expected.owner.clone(),
                        position: expected.position,
                        name: String::new(),
                    },
                    kagari_hir::types::TypeId::Generic(kagari_hir::types::GenericParameterType {
                        owner: actual.owner.clone(),
                        position: actual.position,
                        name: String::new(),
                    }),
                )
            }),
    );
    let expected = |ty: &AbiType| {
        catalog.normalize_type(
            &ty.to_checked_type()
                .with_self(&instance.declaration, &table.for_type.to_checked_type())
                .instantiate(&substitution),
        )
    };
    let actual = |ty: &AbiType| catalog.normalize_type(&ty.to_checked_type());
    // A module-only shape check cannot normalize outputs supplied by a dependency.
    // The linked verifier repeats the full comparison with its complete catalog.
    if defer_projection
        && std::iter::once(expected(&declared.return_type))
            .chain(std::iter::once(actual(&implemented.return_type)))
            .chain(declared.params.iter().map(|p| expected(&p.ty)))
            .chain(implemented.params.iter().map(|p| actual(&p.ty)))
            .any(|ty| ty.contains_projection() && !ty.is_unresolved())
    {
        return true;
    }
    let bounds = |function: &FunctionAbi,
                  normalize: &dyn Fn(&AbiType) -> kagari_hir::types::TypeId| {
        function
            .bounds
            .iter()
            .map(|bound| {
                let mut constraints = bound
                    .constraints
                    .iter()
                    .map(|constraint| {
                        Some(match constraint {
                            ConstraintAbi::Standard(value) => ConstraintAbi::Standard(*value),
                            ConstraintAbi::Trait(value) => {
                                let normalized = normalize(&AbiType::Trait(value.clone()));
                                if normalized.is_unresolved() {
                                    return None;
                                }
                                let AbiType::Trait(value) = AbiType::from_checked_type(&normalized)
                                else {
                                    return None;
                                };
                                ConstraintAbi::Trait(value)
                            }
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                constraints.sort();
                let target = normalize(&bound.ty);
                if target.is_unresolved() {
                    return None;
                }
                Some((AbiType::from_checked_type(&target), constraints))
            })
            .collect::<Option<std::collections::BTreeMap<_, _>>>()
    };
    cancel.check().is_ok()
        && bounds(declared, &expected)
            .is_some_and(|declared| Some(declared) == bounds(implemented, &actual))
        && !expected(&declared.return_type).is_unresolved()
        && !actual(&implemented.return_type).is_unresolved()
        && expected(&declared.return_type) == actual(&implemented.return_type)
        && declared
            .params
            .iter()
            .zip(&implemented.params)
            .all(|(declared, implemented)| {
                cancel.check().is_ok()
                    && declared.mutable == implemented.mutable
                    && !expected(&declared.ty).is_unresolved()
                    && !actual(&implemented.ty).is_unresolved()
                    && expected(&declared.ty) == actual(&implemented.ty)
            })
}

fn aggregate_shape_valid(ty: &TypeAbi, cancel: &CancellationToken) -> bool {
    if ty.name.is_empty()
        || match ty.kind {
            TypeAbiKind::Struct => !ty.variants.is_empty(),
            TypeAbiKind::Enum => !ty.fields.is_empty(),
        }
    {
        return false;
    }
    let mut names = HashSet::new();
    ty.fields
        .iter()
        .map(|field| &field.name)
        .chain(ty.variants.iter().map(|variant| &variant.name))
        .all(|name| cancel.check().is_ok() && !name.is_empty() && names.insert(name))
}

fn owner(
    module: &ModuleIdentity,
    parent: &[DefinitionPathSegment],
    kind: DefinitionKind,
    name: &str,
) -> DefinitionId {
    let mut path = parent.to_vec();
    path.push(DefinitionPathSegment {
        kind,
        name: name.into(),
        occurrence: 0,
    });
    DefinitionId {
        module: module.clone(),
        path,
    }
}
fn parameters(
    declared: &[GenericParameterAbi],
    owner: &DefinitionId,
    outer: &Parameters,
) -> Option<Parameters> {
    let mut params = outer.clone();
    for (position, param) in declared.iter().enumerate() {
        if &param.owner != owner
            || param.position != position
            || !params.insert((owner.clone(), position))
        {
            return None;
        }
    }
    Some(params)
}
fn bounds_valid(
    bounds: &[GenericBoundAbi],
    params: &Parameters,
    cancel: &CancellationToken,
) -> bool {
    bounds_valid_in(bounds, params, None, cancel)
}
fn bounds_valid_in(
    bounds: &[GenericBoundAbi],
    params: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
) -> bool {
    if !bounds.windows(2).all(|pair| pair[0].ty < pair[1].ty) {
        return false;
    }
    let mut seen = HashSet::new();
    bounds.iter().all(|bound| {
        type_valid(&bound.ty, params, self_owner, cancel)
            && matches!(
                bound.ty,
                AbiType::Parameter { .. } | AbiType::Projection { .. }
            )
            && seen.insert(&bound.ty)
            && !bound.constraints.is_empty()
            && constraints_valid(&bound.constraints, params, self_owner, cancel)
    })
}

fn constraints_valid(
    constraints: &[ConstraintAbi],
    params: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
) -> bool {
    constraints.windows(2).all(|pair| pair[0] < pair[1])
        && constraints.iter().all(|constraint| match constraint {
            ConstraintAbi::Standard(_) => true,
            ConstraintAbi::Trait(ty) => {
                type_valid(&AbiType::Trait(ty.clone()), params, self_owner, cancel)
            }
        })
}

fn function_valid(
    function: &FunctionAbi,
    module: &ModuleIdentity,
    parent: &[DefinitionPathSegment],
    outer: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
) -> bool {
    let kind = if parent.is_empty() {
        DefinitionKind::Function
    } else {
        DefinitionKind::Method
    };
    let owner = owner(module, parent, kind, &function.name);
    parameters(&function.generic_params, &owner, outer).is_some_and(|params| {
        bounds_valid_in(&function.bounds, &params, self_owner, cancel)
            && signature_valid(function, &params, self_owner, cancel)
    })
}
fn signature_valid(
    function: &FunctionAbi,
    params: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
) -> bool {
    function
        .params
        .iter()
        .map(|param| &param.ty)
        .chain(std::iter::once(&function.return_type))
        .all(|ty| type_valid(ty, params, self_owner, cancel))
}
fn nominal_valid(id: &DefinitionId, kind: DefinitionKind) -> bool {
    !id.module.package.0.is_empty()
        && !id.module.path.is_empty()
        && !id.module.path.iter().any(String::is_empty)
        && id
            .path
            .last()
            .is_some_and(|part| part.kind == kind && !part.name.is_empty())
}
fn type_valid(
    ty: &AbiType,
    params: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() {
            return false;
        }
        match ty {
            AbiType::Projection {
                receiver,
                interface,
                member,
                arguments,
            } => {
                if params.is_empty() && self_owner.is_none() {
                    return false;
                }
                if *member
                    != kagari_hir::types::associated_type_id(
                        &interface.declaration,
                        member.path.last().map_or("", |p| p.name.as_str()),
                    )
                    || !nominal_valid(&interface.declaration, DefinitionKind::Trait)
                {
                    return false;
                }
                pending.push(receiver);
                pending.extend(arguments);
                for (binding, value) in &interface.associated_types {
                    if *binding
                        != kagari_hir::types::associated_type_id(
                            &interface.declaration,
                            binding.path.last().map_or("", |part| part.name.as_str()),
                        )
                    {
                        return false;
                    }
                    pending.push(value);
                }
                pending.extend(&interface.arguments);
            }
            AbiType::Parameter { owner, position } => {
                if !params.contains(&(owner.clone(), *position)) {
                    return false;
                }
            }
            AbiType::SelfType(owner) => {
                if Some(owner) != self_owner {
                    return false;
                }
            }
            AbiType::Builtin(_) => {}
            AbiType::Host(id) => {
                if kagari_common::host_interface::validate_host_type_identity(id).is_err() {
                    return false;
                }
            }
            AbiType::Tuple(types) => pending.extend(types),
            AbiType::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            AbiType::Array(ty, _) | AbiType::Set(ty, _) | AbiType::Cursor(ty) => pending.push(ty),
            AbiType::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            AbiType::StandardEnum { kind, args } => {
                let count = match kind {
                    StandardEnumKind::Ordering => 0,
                    StandardEnumKind::Option => 1,
                    StandardEnumKind::Result => 2,
                };
                if args.len() != count {
                    return false;
                }
                pending.extend(args);
            }
            AbiType::Struct(ty) | AbiType::Enum(ty) | AbiType::Trait(ty) => {
                if ty.declaration.module.package.0 == "kagari-std" {
                    let Some(kind) =
                        kagari_hir::builtin::traits::StandardTrait::from_id(&ty.declaration)
                    else {
                        return false;
                    };
                    if ty.arguments.len() != kind.contract().generic_params.len()
                        || ty
                            .associated_types
                            .keys()
                            .any(|id| !kind.contract().associated_types.contains_key(id))
                    {
                        return false;
                    }
                }
                pending.extend(&ty.arguments);
                if !matches!(
                    ty.declaration.path.last().map(|part| part.kind),
                    Some(DefinitionKind::Trait)
                ) && !ty.associated_types.is_empty()
                {
                    return false;
                }
                for (member, value) in &ty.associated_types {
                    if *member
                        != kagari_hir::types::associated_type_id(
                            &ty.declaration,
                            member.path.last().map_or("", |p| p.name.as_str()),
                        )
                    {
                        return false;
                    }
                    pending.push(value);
                }
            }
        }
        let nominal = match ty {
            AbiType::Struct(n) => Some((n, DefinitionKind::Struct)),
            AbiType::Enum(n) => Some((n, DefinitionKind::Enum)),
            AbiType::Trait(n) => Some((n, DefinitionKind::Trait)),
            _ => None,
        };
        if let Some((nominal, kind)) = nominal
            && !nominal_valid(&nominal.declaration, kind)
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{BytecodeVerificationError, verify_module};

    #[test]
    fn interface_method_contract_substitutes_self_and_method_binders_inside_containers() {
        let module = ModuleIdentity::single_file("interface.kgr");
        let trait_owner = owner(&module, &[], DefinitionKind::Trait, "Read");
        let impl_owner = owner(&module, &[], DefinitionKind::Impl, "");
        let trait_method = owner(&module, &trait_owner.path, DefinitionKind::Method, "read");
        let impl_method = owner(&module, &impl_owner.path, DefinitionKind::Method, "read");
        let for_type = AbiType::Struct(NominalAbiType {
            associated_types: Default::default(),
            declaration: owner(&module, &[], DefinitionKind::Struct, "Player"),
            arguments: Vec::new(),
        });
        let mut declared = FunctionAbi {
            name: "read".into(),
            generic_params: vec![GenericParameterAbi {
                owner: trait_method.clone(),
                position: 0,
            }],
            bounds: Vec::new(),
            params: vec![ParameterAbi {
                name: "input".into(),
                ty: AbiType::Array(
                    Box::new(AbiType::Tuple(vec![
                        AbiType::SelfType(trait_owner.clone()),
                        AbiType::Parameter {
                            owner: trait_method.clone(),
                            position: 0,
                        },
                    ])),
                    CollectionAccess::Mutable,
                ),
                mutable: false,
            }],
            return_type: AbiType::Builtin(BuiltinType::I32),
        };
        let mut implemented = FunctionAbi {
            name: "read".into(),
            generic_params: vec![GenericParameterAbi {
                owner: impl_method.clone(),
                position: 0,
            }],
            bounds: Vec::new(),
            params: vec![ParameterAbi {
                name: "renamed".into(),
                ty: AbiType::Array(
                    Box::new(AbiType::Tuple(vec![
                        for_type.clone(),
                        AbiType::Parameter {
                            owner: impl_method.clone(),
                            position: 0,
                        },
                    ])),
                    CollectionAccess::Mutable,
                ),
                mutable: false,
            }],
            return_type: AbiType::Builtin(BuiltinType::I32),
        };
        let cancel = CancellationToken::default();
        let trait_instance = NominalAbiType {
            declaration: trait_owner.clone(),
            arguments: Vec::new(),
            associated_types: Default::default(),
        };
        let make_table = || InterfaceTableAbi {
            name: "Read".into(),
            associated_type_families: Vec::new(),
            associated_consts: Vec::new(),
            declaration: impl_owner.clone(),
            for_type: for_type.clone(),
            trait_type: AbiType::Trait(trait_instance.clone()),
            generic_params: Vec::new(),
            bounds: Vec::new(),
            methods: Vec::new(),
            host_bridge: false,
        };
        assert!(same_method_contract(
            &declared,
            &implemented,
            &trait_instance,
            &make_table(),
            &cancel,
        ));
        let original_param = implemented.params[0].ty.clone();
        implemented.params[0].ty = AbiType::Array(
            Box::new(AbiType::Tuple(vec![
                for_type.clone(),
                AbiType::Builtin(BuiltinType::Bool),
            ])),
            CollectionAccess::Mutable,
        );
        assert!(!same_method_contract(
            &declared,
            &implemented,
            &trait_instance,
            &make_table(),
            &cancel,
        ));
        implemented.params[0].ty = original_param;
        declared.bounds.push(GenericBoundAbi {
            ty: AbiType::Parameter {
                owner: trait_method.clone(),
                position: 0,
            },
            constraints: vec![ConstraintAbi::Standard(
                kagari_hir::builtin::surface::StandardTypeConstraint::HashKey,
            )],
        });
        implemented.bounds.push(GenericBoundAbi {
            ty: AbiType::Parameter {
                owner: impl_method.clone(),
                position: 0,
            },
            constraints: declared.bounds[0].constraints.clone(),
        });
        assert!(same_method_contract(
            &declared,
            &implemented,
            &trait_instance,
            &make_table(),
            &cancel,
        ));
        implemented.bounds[0].constraints.clear();
        assert!(!same_method_contract(
            &declared,
            &implemented,
            &trait_instance,
            &make_table(),
            &cancel,
        ));
        let marker = owner(&module, &[], DefinitionKind::Trait, "Marker");
        let applied = |parameter_owner| {
            ConstraintAbi::Trait(NominalAbiType {
                associated_types: Default::default(),
                declaration: marker.clone(),
                arguments: vec![AbiType::Array(
                    Box::new(AbiType::Parameter {
                        owner: parameter_owner,
                        position: 0,
                    }),
                    CollectionAccess::Mutable,
                )],
            })
        };
        declared.bounds[0].constraints = vec![applied(trait_method.clone())];
        implemented.bounds[0].constraints = vec![applied(impl_method)];
        assert!(same_method_contract(
            &declared,
            &implemented,
            &trait_instance,
            &make_table(),
            &cancel,
        ));
        let ConstraintAbi::Trait(instance) = &mut implemented.bounds[0].constraints[0] else {
            unreachable!()
        };
        instance.arguments[0] = AbiType::Array(
            Box::new(AbiType::Builtin(BuiltinType::Bool)),
            CollectionAccess::Mutable,
        );
        assert!(!same_method_contract(
            &declared,
            &implemented,
            &trait_instance,
            &make_table(),
            &cancel,
        ));
    }

    #[test]
    fn interface_tables_require_distinct_local_impl_identities() {
        let original = crate::tests::common::bytecode_ok(
            "struct Player { val value: i32 } pub trait Display { fn show(self) -> i32; } impl Display for Player { fn show(self) -> i32 { self.value } } fn main() -> i32 { 1 }",
        );
        let table_index = original
            .public_items
            .iter()
            .position(|item| matches!(item, PublicAbiItem::InterfaceTable(_)))
            .expect("checked interface table");
        let PublicAbiItem::InterfaceTable(table) = &original.public_items[table_index] else {
            unreachable!()
        };
        assert_eq!(table.declaration.module, original.identity);
        assert_eq!(table.declaration.path.len(), 1);
        assert_eq!(table.declaration.path[0].kind, DefinitionKind::Impl);
        assert!(table.declaration.path[0].name.is_empty());
        for corruption in 0..3 {
            let mut module = original.clone();
            let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
                unreachable!()
            };
            match corruption {
                0 => table.declaration.module.package.0 = "foreign".into(),
                1 => table.declaration.path[0].kind = DefinitionKind::Trait,
                _ => table.declaration.path[0].name = "fabricated".into(),
            }
            assert!(matches!(
                verify_module(&module),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ));
        }
        for corruption in 0..3 {
            let mut module = original.clone();
            let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
                unreachable!()
            };
            match corruption {
                0 => table.methods[0].return_type = AbiType::Builtin(BuiltinType::Bool),
                1 => table.methods[0].params[0].mutable = true,
                _ => table.methods[0].params[0].ty = AbiType::Builtin(BuiltinType::I32),
            }
            assert!(matches!(
                verify_module(&module),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ));
        }
        let mut wrong_trait = original.clone();
        let PublicAbiItem::InterfaceTable(table) = &mut wrong_trait.public_items[table_index]
        else {
            unreachable!()
        };
        let AbiType::Trait(reference) = &mut table.trait_type else {
            unreachable!()
        };
        reference.declaration.path[0].occurrence = 1;
        assert!(matches!(
            verify_module(&wrong_trait),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
        let mut duplicate = original.clone();
        duplicate
            .public_items
            .push(duplicate.public_items[table_index].clone());
        assert!(matches!(
            verify_module(&duplicate),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
        for corruption in 0..3 {
            let mut module = original.clone();
            let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
                unreachable!()
            };
            match corruption {
                0 => table.methods.clear(),
                1 => table.methods[0].name = "other".into(),
                _ => table.methods.push(table.methods[0].clone()),
            }
            assert!(matches!(
                verify_module(&module),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ));
        }
    }

    #[test]
    fn public_signatures_reject_foreign_parameters_invalid_arity_and_escaped_self() {
        let original = crate::tests::common::bytecode_ok(
            "pub fn plain() -> i32 { 1 } pub trait Identity { fn same<T: Eq + Hash + PartialEq>(self, value: T) -> T; }",
        );
        for corruption in 0..8 {
            let mut module = original.clone();
            let (functions, traits) = module.public_items.split_at_mut(1);
            let PublicAbiItem::Function(function) = &mut functions[0] else {
                panic!("public function")
            };
            let PublicAbiItem::Trait(interface) = &mut traits[0] else {
                panic!("public trait")
            };
            match corruption {
                0 => {
                    interface.methods[0].generic_params[0]
                        .owner
                        .module
                        .package
                        .0 = "foreign".into()
                }
                1 => {
                    if let AbiType::Parameter { position, .. } =
                        &mut interface.methods[0].params[1].ty
                    {
                        *position = 99;
                    }
                }
                2 => {
                    function.return_type = AbiType::SelfType(owner(
                        &module.identity,
                        &[],
                        DefinitionKind::Trait,
                        "Identity",
                    ))
                }
                3 => {
                    function.return_type = AbiType::StandardEnum {
                        kind: StandardEnumKind::Result,
                        args: vec![AbiType::Builtin(BuiltinType::I32)],
                    }
                }
                4 => {
                    function.return_type = AbiType::Struct(NominalAbiType {
                        associated_types: Default::default(),
                        declaration: owner(
                            &module.identity,
                            &[],
                            DefinitionKind::Trait,
                            "Identity",
                        ),
                        arguments: vec![],
                    })
                }
                5 => function.generic_params.push(GenericParameterAbi {
                    owner: owner(&module.identity, &[], DefinitionKind::Function, "plain"),
                    position: 0,
                }),
                6 => interface.methods[0].bounds[0].constraints.reverse(),
                _ => {
                    let constraint = interface.methods[0].bounds[0].constraints[0].clone();
                    interface.methods[0].bounds[0]
                        .constraints
                        .insert(0, constraint);
                }
            }
            assert!(
                matches!(
                    verify_module(&module),
                    Err(BytecodeVerificationError::InvalidPublicAbi)
                ),
                "corruption {corruption}"
            );
        }
        let mut module = original;
        let PublicAbiItem::Trait(interface) = &mut module.public_items[1] else {
            panic!("public trait")
        };
        interface.methods[0].return_type =
            AbiType::SelfType(owner(&module.identity, &[], DefinitionKind::Trait, "Other"));
        assert!(matches!(
            verify_module(&module),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
}
