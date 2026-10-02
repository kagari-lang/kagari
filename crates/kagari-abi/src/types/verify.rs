//! Validate serialized semantic types independently of display strings.
use crate::{
    callable::CallableImplementation,
    layout::LayoutValidationError,
    scalar::BuiltinType,
    types::{
        AbiType, ConstraintAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi,
        InterfaceTableAbi, NativeDeclaration, NominalAbiType, PublicAbiItem, TraitAbi,
        TraitContract, TypeAbi, TypeAbiKind, matching,
        substitution::{TypeSubstitution, TypeTransformError, normalize_projections},
    },
};

use kagari_common::{
    cancellation::CancellationToken,
    host_interface,
    identity::{self, DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity},
    range::RangeKind,
};

#[cfg(test)]
use kagari_common::collection::CollectionAccess;

use std::{
    collections::{BTreeMap, HashSet},
    iter,
};

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

pub fn concrete_type_valid(ty: &AbiType, cancel: &CancellationToken) -> bool {
    type_valid(ty, &Parameters::new(), None, cancel)
}

pub fn validate(
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
                (matches!(function.implementation, CallableImplementation::Native(_))
                    || (function.generic_params.is_empty() && function.bounds.is_empty()))
                    && function_valid(function, module, &[], &Parameters::new(), None, cancel)
            }
            PublicAbiItem::Const(value) => type_valid(&value.ty, &Parameters::new(), None, cancel),
            PublicAbiItem::Type(ty) => {
                let kind = match ty.kind {
                    TypeAbiKind::Struct => DefinitionKind::Struct,
                    TypeAbiKind::Enum => DefinitionKind::Enum,
                    TypeAbiKind::Native(kind) => kind.declaration_kind(),
                    TypeAbiKind::NativeStorage(_) => DefinitionKind::AssociatedType,
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

/// Validate declaration binders before any concrete native import can use them.
/// The import separately matches this template and the engine's storage guard.
pub fn validate_native_declarations(
    declarations: &[NativeDeclaration],
    module: &ModuleIdentity,
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    let mut identities = HashSet::new();
    for declaration in declarations {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let id = &declaration.declaration;
        let function = &declaration.function;
        if id.module != *module
            || !id.within_path_limit()
            || !identities.insert(id)
            || !id.path.last().is_some_and(|part| {
                matches!(part.kind, DefinitionKind::Function | DefinitionKind::Method)
                    && part.name == function.name
            })
            || !matches!(
                &function.implementation,
                CallableImplementation::Native(binding)
                    if binding.within_path_limit()
                        && (nominal_valid(binding, DefinitionKind::Function)
                            || nominal_valid(binding, DefinitionKind::Method))
            )
        {
            return Err(LayoutValidationError::Invalid);
        }
        let mut owners = BTreeMap::<_, Vec<_>>::new();
        let mut params = Parameters::new();
        for parameter in &function.generic_params {
            if parameter.owner.module != *module
                || !id.path.starts_with(&parameter.owner.path)
                || !params.insert((parameter.owner.clone(), parameter.position))
            {
                return Err(LayoutValidationError::Invalid);
            }
            owners
                .entry(&parameter.owner)
                .or_default()
                .push(parameter.position);
        }
        if owners
            .values()
            .any(|positions| positions.iter().copied().ne(0..positions.len()))
        {
            return Err(LayoutValidationError::Invalid);
        }
        let mut receiver = id.clone();
        receiver.path.pop();
        let self_owner = receiver
            .path
            .last()
            .is_some_and(|part| part.kind == DefinitionKind::Trait)
            .then_some(&receiver);
        if !bounds_valid_in(&function.bounds, &params, self_owner, cancel)
            || !signature_valid(function, &params, self_owner, cancel)
            || declaration.callable_requirements.len() > 4096
        {
            return Err(LayoutValidationError::Invalid);
        }
        for required in &declaration.callable_requirements {
            let mut owner = required.member.clone();
            let member = owner.path.pop();
            if owner != required.interface.declaration
                || !member.is_some_and(|part| {
                    part.kind == DefinitionKind::Method
                        && part.occurrence == 0
                        && !part.name.is_empty()
                })
                || !required.member.within_path_limit()
                || required.arguments.len() > 4096
                || !type_valid(&required.receiver, &params, self_owner, cancel)
                || !type_valid(
                    &AbiType::Trait(required.interface.clone()),
                    &params,
                    self_owner,
                    cancel,
                )
                || required
                    .arguments
                    .iter()
                    .any(|ty| !type_valid(ty, &params, self_owner, cancel))
            {
                return Err(LayoutValidationError::Invalid);
            }
        }
    }
    Ok(())
}

pub fn validate_trait_contracts(
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
                    && member.declaration == identity::associated_const_id(&owner, name)
                    && scalar_const_type(&member.ty)
                    && member
                        .default_value
                        .as_ref()
                        .is_none_or(|value| scalar_const_valid(&member.ty, value))
            })
        }
        && {
            let mut members = HashSet::new();
            ty.associated_types.iter().all(|member| {
                members.insert(&member.declaration)
                    && member.declaration
                        == identity::associated_type_id(
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
                        && (method.method_policy.override_allowed
                            || !matches!(method.implementation, CallableImplementation::Required))
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

/// Engine defaults are declared once on their canonical protocol. They need no
/// per-implementation ABI entry, especially when their bounds do not hold here.
fn required_methods_present(table: &InterfaceTableAbi, interface: &TraitAbi) -> bool {
    interface.methods.iter().all(|method| {
        table
            .methods
            .iter()
            .any(|actual| actual.name == method.name)
    })
}

pub fn interface_contract_matches(
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

pub fn interface_constants_match(table: &InterfaceTableAbi, interface: &TraitAbi) -> bool {
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
        let actual_parameters = family
            .generic_params
            .iter()
            .map(GenericParameterAbi::as_type)
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
    let normalize = |ty: &AbiType| {
        normalize_projections(
            ty,
            &|interface, receiver, member, arguments| {
                let AbiType::Trait(implemented) = &table.trait_type else {
                    return Err(TypeTransformError::InvalidContract);
                };
                matching::projection_output(
                    matching::ImplementationPattern {
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
    table: &InterfaceTableAbi,
    interface: &TraitAbi,
    normalize: &dyn Fn(&AbiType) -> Result<AbiType, TypeTransformError>,
    cancel: &CancellationToken,
) -> bool {
    let AbiType::Trait(instance) = &table.trait_type else {
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
    declared: &FunctionAbi,
    implemented: &FunctionAbi,
    instance: &NominalAbiType,
    table: &InterfaceTableAbi,
    normalize: &dyn Fn(&AbiType) -> Result<AbiType, TypeTransformError>,
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
        .map(GenericParameterAbi::as_type)
        .collect::<Vec<_>>();
    let mut parameters = TypeSubstitution::for_owner(&instance.declaration, &instance.arguments);
    for (expected, actual) in declared.generic_params.iter().zip(&actual_parameters) {
        parameters.bind(&expected.owner, expected.position, actual);
    }
    let mut receiver = TypeSubstitution::default();
    receiver.bind_receiver(&instance.declaration, &table.for_type);
    let expected = |ty: &AbiType| {
        let ty = receiver.apply(ty, cancel)?;
        let ty = parameters.apply(&ty, cancel)?;
        normalize(&ty)
    };
    let actual = |ty: &AbiType| normalize(ty);
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
                            actual(implemented).is_ok_and(|implemented| declared == implemented)
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
    if defer_projection && signatures.iter().any(AbiType::contains_projection) {
        return true;
    }
    let bounds =
        |function: &FunctionAbi,
         normalize: &dyn Fn(&AbiType) -> Result<AbiType, TypeTransformError>| {
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
                                    let AbiType::Trait(value) =
                                        normalize(&AbiType::Trait(value.clone())).ok()?
                                    else {
                                        return None;
                                    };
                                    ConstraintAbi::Trait(value)
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

fn aggregate_shape_valid(ty: &TypeAbi, cancel: &CancellationToken) -> bool {
    if ty.name.is_empty()
        || match ty.kind {
            TypeAbiKind::Struct => !ty.variants.is_empty(),
            TypeAbiKind::Enum => !ty.fields.is_empty(),
            TypeAbiKind::Native(kind) => !kind.shape_valid(ty),
            TypeAbiKind::NativeStorage(layout) => {
                !ty.fields.is_empty()
                    || !ty.variants.is_empty()
                    || !layout.valid_parameters(ty.generic_params.len())
            }
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
            && seen.insert(&bound.ty)
            && !bound.constraints.is_empty()
            && constraints_valid(&bound.constraints, params, self_owner, cancel)
    })
}

pub(crate) fn native_bounds_valid(
    bounds: &[GenericBoundAbi],
    parameters: &[GenericParameterAbi],
    cancel: &CancellationToken,
) -> bool {
    let params = parameters
        .iter()
        .map(|param| (param.owner.clone(), param.position))
        .collect();
    bounds.len() <= 4096
        && parameters.len() <= 4096
        && bounds.iter().all(|bound| bound.constraints.len() <= 4096)
        && bounds_valid(bounds, &params, cancel)
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
    let implementation_valid = match &function.implementation {
        CallableImplementation::Required => parent
            .last()
            .is_some_and(|owner| owner.kind == DefinitionKind::Trait),
        CallableImplementation::Script => true,
        // Provider authentication and signature matching are linked-program checks.
        CallableImplementation::Native(id) => {
            (nominal_valid(id, DefinitionKind::Function)
                || nominal_valid(id, DefinitionKind::Method))
                && id.within_path_limit()
        }
        CallableImplementation::NativeDefault(application) => {
            parent.last().is_some_and(|owner| {
                matches!(owner.kind, DefinitionKind::Trait | DefinitionKind::Impl)
            }) && application.declaration.within_path_limit()
                && (nominal_valid(&application.declaration, DefinitionKind::Function)
                    || nominal_valid(&application.declaration, DefinitionKind::Method))
                && application.arguments.len() <= 4096
        }
    };
    if function.name.is_empty()
        || !implementation_valid
        || (!function.method_policy.override_allowed
            && !matches!(
                function.implementation,
                CallableImplementation::NativeDefault(_)
            )
            && !parent
                .last()
                .is_some_and(|owner| owner.kind == DefinitionKind::Trait))
    {
        return false;
    }
    let kind = if parent.is_empty() {
        DefinitionKind::Function
    } else {
        DefinitionKind::Method
    };
    let owner = owner(module, parent, kind, &function.name);
    parameters(&function.generic_params, &owner, outer).is_some_and(|params| {
        bounds_valid_in(&function.bounds, &params, self_owner, cancel)
            && signature_valid(function, &params, self_owner, cancel)
            && match &function.implementation {
                CallableImplementation::NativeDefault(application) => application
                    .arguments
                    .iter()
                    .all(|argument| type_valid(argument, &params, self_owner, cancel)),
                _ => true,
            }
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
        .chain(iter::once(&function.return_type))
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
/// Check portable type expressions against an explicitly supplied binder scope.
pub fn types_in_scope<'a>(
    types: impl IntoIterator<Item = &'a AbiType>,
    parameters: &[GenericParameterAbi],
    cancel: &CancellationToken,
) -> bool {
    let parameters = parameters
        .iter()
        .map(|parameter| (parameter.owner.clone(), parameter.position))
        .collect();
    types
        .into_iter()
        .all(|ty| ty.within_wire_limits() && type_valid(ty, &parameters, None, cancel))
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
                    != identity::associated_type_id(
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
                        != identity::associated_type_id(
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
                if host_interface::validate_host_type_identity(id).is_err() {
                    return false;
                }
            }
            AbiType::Tuple(types) => pending.extend(types),
            AbiType::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            AbiType::Range(ty, kind) => {
                if *kind == RangeKind::Full {
                    if **ty != AbiType::Builtin(BuiltinType::Unit) {
                        return false;
                    }
                } else {
                    match ty.as_ref() {
                        AbiType::Builtin(t) if t.integer_layout().is_some() => {}
                        AbiType::Parameter { .. }
                        | AbiType::Projection { .. }
                        | AbiType::SelfType(_) => pending.push(ty),
                        _ => return false,
                    }
                }
            }
            AbiType::Array(ty, _) | AbiType::Set(ty, _) | AbiType::Iter(ty) => pending.push(ty),
            AbiType::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            AbiType::StandardEnum { kind, args } => {
                if args.len() != kind.arity() {
                    return false;
                }
                pending.extend(args);
            }
            AbiType::Struct(nominal)
            | AbiType::NativeObject(nominal)
            | AbiType::Enum(nominal)
            | AbiType::Trait(nominal) => {
                pending.extend(&nominal.arguments);
                if !matches!(
                    nominal.declaration.path.last().map(|part| part.kind),
                    Some(DefinitionKind::Trait)
                ) && !nominal.associated_types.is_empty()
                {
                    return false;
                }
                for (member, value) in &nominal.associated_types {
                    if *member
                        != identity::associated_type_id(
                            &nominal.declaration,
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
            AbiType::NativeObject(n) => Some((n, DefinitionKind::AssociatedType)),
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
mod tests;
