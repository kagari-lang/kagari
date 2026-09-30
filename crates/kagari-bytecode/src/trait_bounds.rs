//! Recheck associated outputs and host trait bounds against the dependency closure.
mod applications;
mod associated;

use crate::{
    BytecodeInstruction, BytecodeModule, BytecodeProgram,
    trait_bounds::associated::{associated_bounds_match, host_bounds_match},
};
use kagari_abi::{
    standard::traits::StandardTrait,
    types::{
        self as abi, AbiType, GenericBoundAbi, GenericParameterAbi, NominalAbiType, PublicAbiItem,
        TraitAbi, inheritance as trait_inheritance,
        proofs::{ProofCatalog, host_application},
        substitution::TypeTransformError,
        verify,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
};

fn contract<'a>(id: &DefinitionId, closure: &[&'a BytecodeModule]) -> Option<&'a TraitAbi> {
    let owner = closure.iter().find(|module| module.identity == id.module)?;
    abi::trait_contract(
        &owner.identity,
        &owner.public_items,
        &owner.trait_contracts,
        id,
    )
}

/// Read the bounded applied parent closure from portable trait contracts.
pub fn interface_ancestors(
    interface: &NominalAbiType,
    receiver: &AbiType,
    closure: &[&BytecodeModule],
) -> Option<Vec<NominalAbiType>> {
    trait_inheritance::trait_closure(interface, receiver, &CancellationToken::default(), &|id| {
        contract(id, closure)
    })
    .ok()
}

fn executable_interface(
    applied: &NominalAbiType,
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
        if StandardTrait::from_id(&view.declaration).is_some_and(|kind| !kind.dynamic()) {
            return false;
        }
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
            if abi::interface_method_types(
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

fn declarations(module: &BytecodeModule) -> impl Iterator<Item = (DefinitionId, &TraitAbi)> {
    let public = module.public_items.iter().filter_map(|item| {
        let PublicAbiItem::Trait(record) = item else {
            return None;
        };
        Some((
            DefinitionId {
                module: module.identity.clone(),
                path: vec![DefinitionPathSegment {
                    kind: DefinitionKind::Trait,
                    name: record.name.clone(),
                    occurrence: 0,
                }],
            },
            record,
        ))
    });
    public.chain(
        module
            .trait_contracts
            .iter()
            .map(|record| (record.declaration.clone(), &record.abi)),
    )
}

pub(super) fn trait_bounds_match(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    program: Option<&BytecodeProgram>,
) -> bool {
    linked_bounds_match(module, closure, program).unwrap_or(false)
}

fn linked_bounds_match(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    program: Option<&BytecodeProgram>,
) -> Result<bool, TypeTransformError> {
    let cancel = CancellationToken::default();
    applications::validate(module, closure, &cancel)?;
    let tables: Vec<_> = closure
        .iter()
        .flat_map(|dependency| &dependency.public_items)
        .filter_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) if !table.host_bridge && !table.native_bridge => {
                Some(table.as_ref())
            }
            _ => None,
        })
        .collect();
    let catalog = ProofCatalog::new(
        tables.clone(),
        closure
            .iter()
            .flat_map(|module| &module.host_interface.types)
            .collect(),
        closure.iter().flat_map(|module| &module.enumerations),
        closure.iter().flat_map(|module| declarations(module)),
        &cancel,
    )?;
    if !catalog.overrides_valid(&cancel)? || !instruction_contracts_match(module, closure, program)
    {
        return Ok(false);
    }
    for import in &module.engine_imports {
        let Some(owner) = closure
            .iter()
            .find(|owner| owner.identity == import.instance.declaration.module)
        else {
            return Ok(false);
        };
        let Some(declaration) = owner
            .native_declarations
            .iter()
            .find(|declaration| declaration.declaration == import.instance.declaration)
        else {
            return Ok(false);
        };
        if !import.matches_declaration(
            declaration,
            &catalog,
            |id| {
                closure
                    .iter()
                    .find(|owner| owner.identity == id.module)?
                    .public_items
                    .iter()
                    .find_map(|item| {
                        if let PublicAbiItem::InterfaceTable(table) = item {
                            (table.declaration == *id).then_some(table.as_ref())
                        } else {
                            None
                        }
                    })
            },
            &cancel,
        )? {
            return Ok(false);
        }
    }
    // Validate unused declaration graphs and constructor references too.
    for (id, record) in closure.iter().flat_map(|module| declarations(module)) {
        let applied = NominalAbiType {
            declaration: id.clone(),
            arguments: record
                .generic_params
                .iter()
                .map(GenericParameterAbi::as_type)
                .collect(),
            associated_types: Default::default(),
        };
        catalog.ancestry(&applied, &AbiType::SelfType(id), &cancel)?;
    }
    for table in tables {
        let AbiType::Trait(applied) = &table.trait_type else {
            return Ok(false);
        };
        if !parents_proven(
            applied,
            &table.for_type,
            &table.bounds,
            &catalog,
            closure,
            &cancel,
        )? {
            return Ok(false);
        }
    }
    for host in closure
        .iter()
        .flat_map(|module| &module.host_interface.types)
    {
        for implementation in &host.trait_implementations {
            if !parents_proven(
                &host_application(implementation),
                &AbiType::Host(host.id.clone()),
                &[],
                &catalog,
                closure,
                &cancel,
            )? {
                return Ok(false);
            }
        }
    }
    for item in &module.public_items {
        let PublicAbiItem::InterfaceTable(table) = item else {
            continue;
        };
        let AbiType::Trait(interface) = &table.trait_type else {
            return Ok(false);
        };
        let Some(record) = contract(&interface.declaration, closure) else {
            return Ok(false);
        };
        if interface.associated_types.len()
            != record
                .associated_types
                .iter()
                .filter(|member| member.generic_params.is_empty())
                .count()
            || !verify::interface_constants_match(table, record)
            || !verify::interface_families_match(table, record, &cancel)
            || !verify::interface_methods_match(
                table,
                record,
                &|ty| catalog.normalize(ty, &cancel),
                &cancel,
            )
        {
            return Ok(false);
        }
        if !table.host_bridge
            && matches!(table.for_type, AbiType::Host(_))
            && catalog.implementation_count(interface, &table.for_type, &[], &cancel)? != 1
        {
            return Ok(false);
        }
        if table.native_bridge && !catalog.holds(interface, &table.for_type, &[], &cancel)? {
            return Ok(false);
        }
        if !associated_bounds_match(table, interface, record, &catalog, closure, &cancel)? {
            return Ok(false);
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
            return Ok(false);
        };
        let AbiType::Trait(interface) = &table.trait_type else {
            return Ok(false);
        };
        if !table.native_bridge
            && catalog.implementation_count(interface, &table.for_type, &[], &cancel)? != 1
        {
            return Ok(false);
        }
    }
    host_bounds_match(module, closure, &catalog, &cancel)
}

fn parents_proven(
    interface: &NominalAbiType,
    receiver: &AbiType,
    bounds: &[GenericBoundAbi],
    catalog: &ProofCatalog<'_>,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    // Offline host declarations may advertise traits outside this program.
    if matches!(receiver, AbiType::Host(_))
        && StandardTrait::from_id(&interface.declaration).is_none()
        && contract(&interface.declaration, closure).is_none()
    {
        return Ok(true);
    }
    let bounds = catalog.expand_bounds(bounds, cancel)?;
    for parent in catalog
        .ancestry(interface, receiver, cancel)?
        .into_iter()
        .skip(1)
    {
        if !catalog.holds(&parent, receiver, &bounds, cancel)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn instruction_contracts_match(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    program: Option<&BytecodeProgram>,
) -> bool {
    for instruction in module
        .functions
        .iter()
        .flat_map(|function| &function.instructions)
    {
        if let BytecodeInstruction::UpcastInterface { source, target, .. } = instruction {
            let Some(parents) =
                interface_ancestors(source, &AbiType::Trait(source.clone()), closure)
            else {
                return false;
            };
            if !parents.contains(target) {
                return false;
            }
        }
        if let BytecodeInstruction::MakeInterface {
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
    true
}

#[cfg(test)]
mod tests;
