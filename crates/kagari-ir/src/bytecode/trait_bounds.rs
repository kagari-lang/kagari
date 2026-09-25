//! Recheck concrete host trait bounds against the verified dependency closure.
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

fn signature(table: &InterfaceTableAbi) -> Option<ImplementationSignature> {
    let AbiType::Trait(trait_type) = &table.trait_type else {
        return None;
    };
    let generic_params = table
        .generic_params
        .iter()
        .map(|param| GenericParameterType {
            owner: param.owner.clone(),
            position: param.position,
            name: String::new(),
        })
        .collect::<Vec<_>>();
    let mut bounds = kagari_hir::typeck::GenericBounds::new();
    for bound in &table.bounds {
        let parameter = GenericParameterType {
            owner: bound.owner.clone(),
            position: bound.position,
            name: String::new(),
        };
        bounds.insert(
            parameter,
            bound
                .constraints
                .iter()
                .map(|constraint| match constraint {
                    ConstraintAbi::Standard(standard) => ConstraintTarget::Standard(*standard),
                    ConstraintAbi::Trait(ty) => ConstraintTarget::Trait(ty.to_checked_type()),
                })
                .collect(),
        );
    }
    Some(ImplementationSignature {
        id: table.declaration.clone(),
        trait_type: trait_type.to_checked_type(),
        for_type: table.for_type.to_checked_type(),
        generic_params,
        bounds,
        methods: Default::default(),
    })
}

pub(super) fn host_bounds_match(module: &BytecodeModule, closure: &[&BytecodeModule]) -> bool {
    let mut signatures = Vec::new();
    for dependency in closure {
        for item in &dependency.public_items {
            if let PublicAbiItem::InterfaceTable(table) = item {
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
    let Some(catalog) = AggregateCatalog::from_implementation_signatures(signatures) else {
        return false;
    };
    let cancel = kagari_common::cancellation::CancellationToken::default();
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
            let arguments = implementation
                .trait_arguments
                .iter()
                .map(AbiType::from_host_type)
                .collect::<Vec<_>>();
            for bound in &trait_abi.bounds {
                let Some(argument) = arguments.get(bound.position) else {
                    return false;
                };
                let actual = argument.to_checked_type();
                for constraint in &bound.constraints {
                    let ConstraintAbi::Trait(required) = constraint else {
                        continue;
                    };
                    let Some(required) = AbiType::Trait(required.clone())
                        .instantiate(&implementation.trait_id, &arguments)
                    else {
                        return false;
                    };
                    let TypeId::Trait(required) = required.to_checked_type() else {
                        return false;
                    };
                    let script = catalog.implementation_count_bounded(
                        &required,
                        &actual,
                        MAX_MATCH_CHECKS,
                        MAX_PROOF_DEPTH,
                        &cancel,
                    );
                    let host_count = usize::from(matches!(&actual, TypeId::Host(id)
                    if closure
                        .iter()
                        .flat_map(|dependency| &dependency.host_interface.types)
                        .filter(|candidate| &candidate.id == id)
                        .flat_map(|candidate| &candidate.trait_implementations)
                        .any(|table| {
                            table.trait_id == required.declaration
                                && table
                                    .trait_arguments
                                    .iter()
                                    .map(AbiType::from_host_type)
                                    .map(|argument| argument.to_checked_type())
                                    .collect::<Vec<_>>()
                                    == required.arguments
                        })));
                    if !matches!(script, Ok(count) if count + host_count == 1) {
                        return false;
                    }
                }
            }
        }
    }
    true
}
