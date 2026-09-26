//! Recheck associated outputs and host trait bounds against the dependency closure.
use super::BytecodeModule;
use crate::module::{
    PublicAbiItem,
    abi::{AbiType, ConstraintAbi, InterfaceTableAbi},
};
use kagari_hir::{
    aggregates::{AggregateCatalog, ImplementationSignature},
    typeck::ConstraintTarget,
    types::{GenericParameterType, TypeId},
};

const MAX_IMPLEMENTATIONS: usize = 4096;
const MAX_MATCH_CHECKS: usize = 100_000;
const MAX_PROOF_DEPTH: usize = 64;

fn contract<'a>(
    id: &kagari_common::identity::DefinitionId,
    closure: &[&'a BytecodeModule],
) -> Option<&'a crate::module::abi::TraitAbi> {
    let owner = closure.iter().find(|module| module.identity == id.module)?;
    owner
        .trait_contracts
        .iter()
        .find(|record| &record.declaration == id)
        .map(|record| &record.abi)
        .or_else(|| {
            owner.public_items.iter().find_map(|item| match item {
                PublicAbiItem::Trait(record)
                    if id.path.len() == 1
                        && id.path[0].name == record.name
                        && id.path[0].kind == kagari_common::identity::DefinitionKind::Trait
                        && id.path[0].occurrence == 0 =>
                {
                    Some(record)
                }
                _ => None,
            })
        })
}

fn inheritance(
    interface: &kagari_hir::types::NominalType,
    receiver: &TypeId,
    closure: &[&BytecodeModule],
) -> Option<Vec<kagari_hir::types::NominalType>> {
    kagari_hir::aggregates::trait_inheritance_closure(
        interface,
        receiver,
        &Default::default(),
        &|id| {
            let record = contract(id, closure)?;
            Some((
                record
                    .generic_params
                    .iter()
                    .map(|parameter| GenericParameterType {
                        owner: parameter.owner.clone(),
                        position: parameter.position,
                        name: String::new(),
                    })
                    .collect(),
                record
                    .supertraits
                    .iter()
                    .map(|parent| parent.to_checked_type())
                    .collect(),
            ))
        },
    )
    .ok()
}

/// Read the bounded applied parent closure from portable trait contracts.
pub fn interface_ancestors(
    interface: &crate::module::abi::NominalAbiType,
    receiver: &crate::module::abi::AbiType,
    closure: &[&BytecodeModule],
) -> Option<Vec<crate::module::abi::NominalAbiType>> {
    inheritance(
        &interface.to_checked_type(),
        &receiver.to_checked_type(),
        closure,
    )
    .map(|parents| {
        parents
            .iter()
            .map(crate::module::abi::NominalAbiType::from_checked_type)
            .collect()
    })
}

fn executable_interface(
    applied: &crate::module::abi::NominalAbiType,
    receiver: &AbiType,
    closure: &[&BytecodeModule],
) -> bool {
    let Some(views) = interface_ancestors(applied, &AbiType::Trait(applied.clone()), closure)
    else {
        return false;
    };
    if interface_ancestors(applied, receiver, closure).as_ref() != Some(&views) {
        return false;
    }
    for view in views {
        let Some(record) = contract(&view.declaration, closure) else {
            return false;
        };
        if !record.associated_consts.is_empty()
            || record
                .associated_types
                .iter()
                .any(|member| !member.generic_params.is_empty())
            || view.associated_types.len() != record.associated_types.len()
            || record
                .associated_types
                .iter()
                .any(|member| !view.associated_types.contains_key(&member.declaration))
        {
            return false;
        }
        let Some(owner) = closure
            .iter()
            .find(|module| module.identity == view.declaration.module)
        else {
            return false;
        };
        for slot in 0..record.methods.len() {
            if crate::module::abi::interface_method_types(
                &owner.identity,
                &owner.public_items,
                &owner.trait_contracts,
                &view,
                slot,
            )
            .is_none()
            {
                return false;
            }
        }
    }
    true
}

fn signature(table: &InterfaceTableAbi) -> Option<ImplementationSignature> {
    table.checked_signature()
}

/// Check constructor references even in unused portable templates.
fn projection_uses_valid(
    ty: &TypeId,
    assumptions: Option<&kagari_hir::typeck::GenericBounds>,
    catalog: &AggregateCatalog,
    closure: &[&BytecodeModule],
    cancel: &kagari_common::cancellation::CancellationToken,
) -> bool {
    let mut pending = vec![ty.clone()];
    let mut remaining = 8192usize;
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() || remaining == 0 {
            return false;
        }
        remaining -= 1;
        if let TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } = &ty
        {
            let Some(record) = contract(&interface.declaration, closure) else {
                return false;
            };
            let Some(definition) = record
                .associated_types
                .iter()
                .find(|definition| &definition.declaration == member)
            else {
                return false;
            };
            if definition.generic_params.len() != arguments.len()
                || record.generic_params.len() != interface.arguments.len()
            {
                return false;
            }
            if let Some(assumptions) = assumptions {
                let mut substitution: kagari_hir::types::TypeSubstitution = record
                    .generic_params
                    .iter()
                    .zip(&interface.arguments)
                    .chain(definition.generic_params.iter().zip(arguments))
                    .map(|(parameter, argument)| {
                        (
                            GenericParameterType {
                                owner: parameter.owner.clone(),
                                position: parameter.position,
                                name: String::new(),
                            },
                            argument.clone(),
                        )
                    })
                    .collect();
                substitution
                    .insert_receiver(interface.declaration.clone(), receiver.as_ref().clone());
                for bound in &definition.parameter_bounds {
                    let actual = catalog
                        .normalize_type(&bound.ty.to_checked_type().instantiate(&substitution));
                    for required in &bound.constraints {
                        let valid = match required {
                            ConstraintAbi::Standard(required) => {
                                kagari_hir::typeck::type_satisfies_standard_constraint(
                                    &actual,
                                    *required,
                                    assumptions,
                                )
                            }
                            ConstraintAbi::Trait(required) => {
                                let required =
                                    required.to_checked_type().instantiate(&substitution);
                                assumptions.get(&actual).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(available) if available.satisfies(&required)))) || matches!(catalog.concrete_interface_implementation(&required, &actual, assumptions, MAX_MATCH_CHECKS, MAX_PROOF_DEPTH, cancel), Ok(Some(_)))
                            }
                        };
                        if !valid {
                            return false;
                        }
                    }
                }
            }
        }
        ty.map_children(|child| {
            pending.push(child.clone());
            child.clone()
        });
    }
    true
}

pub(super) fn trait_bounds_match(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    program: Option<&super::BytecodeProgram>,
) -> bool {
    let mut signatures = Vec::new();
    for dependency in closure {
        for item in &dependency.public_items {
            if let PublicAbiItem::InterfaceTable(table) = item
                && !table.host_bridge
            {
                if signatures.len() == MAX_IMPLEMENTATIONS {
                    return false;
                }
                let Some(converted) = signature(table) else {
                    return false;
                };
                signatures.push(converted);
            }
        }
    }
    let mut host_ids = std::collections::HashSet::new();
    for host in closure
        .iter()
        .flat_map(|dependency| &dependency.host_interface.types)
    {
        if !host_ids.insert(&host.id) {
            continue;
        }
        for (index, implementation) in host.trait_implementations.iter().enumerate() {
            if signatures.len() == MAX_IMPLEMENTATIONS {
                return false;
            }
            let mut id = host.id.clone();
            id.path
                .push(kagari_common::identity::DefinitionPathSegment {
                    kind: kagari_common::identity::DefinitionKind::Impl,
                    name: String::new(),
                    occurrence: index as u32,
                });
            signatures.push(ImplementationSignature {
                associated_type_families: Default::default(),
                id,
                trait_type: kagari_hir::host::HostDeclarations::trait_type(implementation),
                for_type: TypeId::Host(host.id.clone()),
                generic_params: Vec::new(),
                bounds: Default::default(),
                methods: Default::default(),
            });
        }
    }
    let Some(catalog) = AggregateCatalog::from_implementation_signatures(signatures) else {
        return false;
    };
    let cancel = kagari_common::cancellation::CancellationToken::default();
    for instruction in module
        .functions
        .iter()
        .flat_map(|function| &function.instructions)
    {
        if let super::BytecodeInstruction::UpcastInterface { source, target, .. } = instruction {
            let Some(parents) =
                interface_ancestors(source, &AbiType::Trait(source.clone()), closure)
            else {
                return false;
            };
            if !parents.contains(target) {
                return false;
            }
        }
        if let super::BytecodeInstruction::MakeInterface {
            module: owner,
            implementation,
            ..
        } = instruction
        {
            let target = program
                .and_then(|program| program.modules.get(owner.index()))
                .or_else(|| (program.is_none() && owner.index() == 0).then_some(module));
            let Some(target) = target else {
                return false;
            };
            let Some(linked) = target.interface_tables.get(implementation.index()) else {
                return false;
            };
            let Some(table) = target.public_items.iter().find_map(|item| match item {
                PublicAbiItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                    table.instantiate(&linked.arguments)
                }
                _ => None,
            }) else {
                return false;
            };
            let AbiType::Trait(applied) = &table.trait_type else {
                return false;
            };
            if !executable_interface(applied, &table.for_type, closure) {
                return false;
            }
            let Some(parents) = interface_ancestors(applied, &table.for_type, closure) else {
                return false;
            };
            for parent in parents.into_iter().skip(1) {
                let exists = closure.iter().any(|owner| {
                    owner.interface_tables.iter().any(|linked| {
                        owner.public_items.iter().any(|item| {
                            let PublicAbiItem::InterfaceTable(template) = item else {
                                return false;
                            };
                            template.declaration == linked.declaration
                                && template.instantiate(&linked.arguments).is_some_and(
                                    |candidate| {
                                        candidate.for_type == table.for_type
                                            && candidate.trait_type
                                                == AbiType::Trait(parent.clone())
                                    },
                                )
                        })
                    })
                });
                if !exists {
                    return false;
                }
            }
        }
    }
    // Validate unused declarations too; cyclic inheritance cannot hide behind
    // the absence of a conversion or method call.
    for member in closure {
        let public = member.public_items.iter().filter_map(|item| {
            let PublicAbiItem::Trait(record) = item else {
                return None;
            };
            Some((
                kagari_common::identity::DefinitionId {
                    module: member.identity.clone(),
                    path: vec![kagari_common::identity::DefinitionPathSegment {
                        kind: kagari_common::identity::DefinitionKind::Trait,
                        name: record.name.clone(),
                        occurrence: 0,
                    }],
                },
                record,
            ))
        });
        let private = member
            .trait_contracts
            .iter()
            .map(|record| (record.declaration.clone(), &record.abi));
        for (id, record) in public.chain(private) {
            let applied = kagari_hir::types::NominalType {
                declaration: id.clone(),
                associated_types: Default::default(),
                arguments: record
                    .generic_params
                    .iter()
                    .map(|parameter| {
                        TypeId::Generic(GenericParameterType {
                            owner: parameter.owner.clone(),
                            position: parameter.position,
                            name: String::new(),
                        })
                    })
                    .collect(),
            };
            if inheritance(&applied, &TypeId::SelfType(id), closure).is_none() {
                return false;
            }
            for method in &record.methods {
                if !projection_uses_valid(
                    &method.return_type.to_checked_type(),
                    None,
                    &catalog,
                    closure,
                    &cancel,
                ) || method.params.iter().any(|parameter| {
                    !projection_uses_valid(
                        &parameter.ty.to_checked_type(),
                        None,
                        &catalog,
                        closure,
                        &cancel,
                    )
                }) {
                    return false;
                }
            }
        }
    }
    for implementation in catalog.implementations() {
        // An offline host declaration can advertise traits outside this program.
        // Those tables become usable only when their declaring module is linked.
        if matches!(implementation.for_type, TypeId::Host(_))
            && contract(&implementation.trait_type.declaration, closure).is_none()
        {
            continue;
        }
        let mut bounds = implementation.bounds.clone();
        for (receiver, constraints) in &implementation.bounds {
            for constraint in constraints {
                if let ConstraintTarget::Trait(applied) = constraint {
                    let Some(parents) = inheritance(applied, receiver, closure) else {
                        return false;
                    };
                    let expanded = bounds.entry(receiver.clone()).or_default();
                    for parent in parents {
                        let constraint = ConstraintTarget::Trait(parent);
                        if !expanded.contains(&constraint) {
                            expanded.push(constraint);
                        }
                    }
                }
            }
        }
        let Some(parents) = inheritance(
            &implementation.trait_type,
            &implementation.for_type,
            closure,
        ) else {
            return false;
        };
        for parent in parents.into_iter().skip(1) {
            if !matches!(
                catalog.concrete_interface_implementation(
                    &parent,
                    &implementation.for_type,
                    &bounds,
                    MAX_MATCH_CHECKS,
                    MAX_PROOF_DEPTH,
                    &cancel
                ),
                Ok(Some(_))
            ) {
                return false;
            }
        }
    }
    for item in &module.public_items {
        let PublicAbiItem::InterfaceTable(table) = item else {
            continue;
        };
        let AbiType::Trait(interface) = &table.trait_type else {
            return false;
        };
        let Some(owner) = closure
            .iter()
            .find(|member| member.identity == interface.declaration.module)
        else {
            return false;
        };
        let contract = owner
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicAbiItem::Trait(contract)
                    if interface
                        .declaration
                        .path
                        .last()
                        .is_some_and(|part| part.name == contract.name) =>
                {
                    Some(contract)
                }
                _ => None,
            })
            .or_else(|| {
                owner
                    .trait_contracts
                    .iter()
                    .find(|contract| contract.declaration == interface.declaration)
                    .map(|contract| &contract.abi)
            });
        let Some(contract) = contract else {
            return false;
        };
        if interface.associated_types.len()
            != contract
                .associated_types
                .iter()
                .filter(|member| member.generic_params.is_empty())
                .count()
            || !crate::module::abi::verify::interface_constants_match(table, contract)
            || !crate::module::abi::verify::interface_families_match(table, contract, &cancel)
            || !crate::module::abi::verify::interface_methods_match(
                table, contract, &catalog, &cancel,
            )
        {
            return false;
        }
        if !table.host_bridge
            && matches!(table.for_type, AbiType::Host(_))
            && !matches!(
                catalog.implementation_count_bounded(
                    &interface.to_checked_type(),
                    &table.for_type.to_checked_type(),
                    MAX_MATCH_CHECKS,
                    MAX_PROOF_DEPTH,
                    &cancel
                ),
                Ok(1)
            )
        {
            return false;
        }
        let checked = interface.to_checked_type();
        let substitution: kagari_hir::types::TypeSubstitution = contract
            .generic_params
            .iter()
            .map(|parameter| GenericParameterType {
                owner: parameter.owner.clone(),
                position: parameter.position,
                name: String::new(),
            })
            .zip(checked.arguments.iter().cloned())
            .collect();
        let bridge = if table.host_bridge {
            signature(table)
        } else {
            None
        };
        let Some(implementation) = catalog
            .implementation_signature(&table.declaration)
            .or(bridge.as_ref())
        else {
            return false;
        };
        for member in &contract.associated_types {
            let mut substitution = substitution.clone();
            let mut available = implementation.bounds.clone();
            let actual = if member.generic_params.is_empty() {
                let Some(actual) = checked.associated_types.get(&member.declaration) else {
                    return false;
                };
                actual.clone()
            } else {
                let Some(family) = implementation
                    .associated_type_families
                    .get(&member.declaration)
                else {
                    return false;
                };
                substitution.extend(
                    member
                        .generic_params
                        .iter()
                        .zip(&family.inputs.parameters)
                        .map(|(parameter, actual)| {
                            (
                                GenericParameterType {
                                    owner: parameter.owner.clone(),
                                    position: parameter.position,
                                    name: String::new(),
                                },
                                TypeId::Generic(actual.clone()),
                            )
                        }),
                );
                for (target, bounds) in &family.inputs.bounds {
                    available
                        .entry(target.clone())
                        .or_default()
                        .extend(bounds.clone());
                }
                for bound in &member.parameter_bounds {
                    available
                        .entry(bound.ty.to_checked_type().instantiate(&substitution))
                        .or_default()
                        .extend(bound.constraints.iter().map(|constraint| match constraint {
                            ConstraintAbi::Standard(value) => ConstraintTarget::Standard(*value),
                            ConstraintAbi::Trait(value) => ConstraintTarget::Trait(
                                value.to_checked_type().instantiate(&substitution),
                            ),
                        }));
                }
                for (receiver, constraints) in available.clone() {
                    for constraint in constraints {
                        if let ConstraintTarget::Trait(applied) = constraint {
                            let Some(parents) = inheritance(&applied, &receiver, closure) else {
                                return false;
                            };
                            let expanded = available.entry(receiver.clone()).or_default();
                            for parent in parents {
                                let constraint = ConstraintTarget::Trait(parent);
                                if !expanded.contains(&constraint) {
                                    expanded.push(constraint);
                                }
                            }
                        }
                    }
                }
                if !projection_uses_valid(
                    &family.value,
                    Some(&available),
                    &catalog,
                    closure,
                    &cancel,
                ) {
                    return false;
                }
                let value = catalog.normalize_type(&family.value);
                if value.is_unresolved() {
                    return false;
                }
                value
            };
            for constraint in &member.bounds {
                let required = match constraint {
                    ConstraintAbi::Standard(value) => ConstraintTarget::Standard(*value),
                    ConstraintAbi::Trait(value) => {
                        let required = TypeId::Trait(value.to_checked_type())
                            .with_self(&checked.declaration, &implementation.for_type)
                            .instantiate(&substitution);
                        let TypeId::Trait(required) = required else {
                            return false;
                        };
                        ConstraintTarget::Trait(required)
                    }
                };
                let proven = match &required {
                    ConstraintTarget::Standard(value) => kagari_hir::typeck::type_satisfies_standard_constraint(&actual, *value, &available),
                    ConstraintTarget::Trait(required) => available.get(&actual).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(actual) if actual.satisfies(required))))
                        || matches!(catalog.concrete_interface_implementation(required, &actual, &available, MAX_MATCH_CHECKS, MAX_PROOF_DEPTH, &cancel), Ok(Some(_))),
                };
                if !proven {
                    return false;
                }
            }
        }
    }
    for linked in &module.interface_tables {
        if linked.arguments.is_empty() {
            continue;
        }
        let Some(table) = module.public_items.iter().find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                table.instantiate(&linked.arguments)
            }
            _ => None,
        }) else {
            return false;
        };
        let TypeId::Trait(interface) = table.trait_type.to_checked_type() else {
            return false;
        };
        if !matches!(
            catalog.implementation_count_bounded(
                &interface,
                &table.for_type.to_checked_type(),
                MAX_MATCH_CHECKS,
                MAX_PROOF_DEPTH,
                &cancel
            ),
            Ok(1)
        ) {
            return false;
        }
    }
    for host in &module.host_interface.types {
        for implementation in &host.trait_implementations {
            let Some(owner) = closure
                .iter()
                .find(|member| member.identity == implementation.trait_id.module)
            else {
                continue;
            };
            let trait_name = implementation.trait_id.path.last().map(|part| &part.name);
            let trait_abi = owner
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicAbiItem::Trait(ty) if Some(&ty.name) == trait_name => Some(ty),
                    _ => None,
                })
                .or_else(|| {
                    owner
                        .trait_contracts
                        .iter()
                        .find(|contract| contract.declaration == implementation.trait_id)
                        .map(|contract| &contract.abi)
                });
            let Some(trait_abi) = trait_abi else {
                return false;
            };
            let applied = kagari_hir::host::HostDeclarations::trait_type(implementation);
            let substitution = trait_abi
                .generic_params
                .iter()
                .map(|param| GenericParameterType {
                    owner: param.owner.clone(),
                    position: param.position,
                    name: String::new(),
                })
                .zip(applied.arguments.iter().cloned())
                .collect();
            let ordinary = trait_abi
                .bounds
                .iter()
                .map(|bound| (bound.ty.to_checked_type(), &bound.constraints));
            let outputs = trait_abi.associated_types.iter().map(|member| {
                (
                    applied
                        .associated_types
                        .get(&member.declaration)
                        .cloned()
                        .unwrap_or(TypeId::Error),
                    &member.bounds,
                )
            });
            for (target, constraints) in ordinary.chain(outputs) {
                let actual = catalog.normalize_type(
                    &target
                        .with_associated_types(&applied)
                        .with_self(&applied.declaration, &TypeId::Host(host.id.clone()))
                        .instantiate(&substitution),
                );
                for constraint in constraints {
                    match constraint {
                        ConstraintAbi::Standard(standard) => {
                            if !kagari_hir::typeck::type_satisfies_standard_constraint(
                                &actual,
                                *standard,
                                &Default::default(),
                            ) {
                                return false;
                            }
                        }
                        ConstraintAbi::Trait(required) => {
                            let required = catalog.normalize_type(
                                &TypeId::Trait(required.to_checked_type())
                                    .with_associated_types(&applied)
                                    .with_self(&applied.declaration, &TypeId::Host(host.id.clone()))
                                    .instantiate(&substitution),
                            );
                            let TypeId::Trait(required) = required else {
                                return false;
                            };
                            if !matches!(
                                catalog.implementation_count_bounded(
                                    &required,
                                    &actual,
                                    MAX_MATCH_CHECKS,
                                    MAX_PROOF_DEPTH,
                                    &cancel
                                ),
                                Ok(1)
                            ) {
                                return false;
                            }
                        }
                    }
                }
            }
        }
    }
    true
}
