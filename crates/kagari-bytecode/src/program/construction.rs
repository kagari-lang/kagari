//! Recheck closed shared construction coverage using executable slots alone.
use crate::{
    artifact::MAX_ARTIFACT_TABLE_RECORDS,
    instruction::{BytecodeInstruction, CallTarget},
    module::CallableTarget,
    program::BytecodeProgram,
    verifier::BytecodeVerificationError,
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionTable};
use kagari_contract::{
    callable::construction::{
        ConstructionBody, ConstructionMethod, ConstructionStep, closed_constructions,
        native_construction,
    },
    types::{ConcreteFunctionIdentity, PublicItem, trait_contract},
};
use kagari_types::ty::{Ty, substitution::TypeSubstitution};

pub(super) fn verify(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), BytecodeVerificationError> {
    let invalid = || BytecodeVerificationError::InvalidInterfaceTable;
    let mut bodies = Vec::new();
    let mut methods = Vec::new();
    for module in &program.modules {
        cancel
            .check()
            .map_err(|_| BytecodeVerificationError::Cancelled)?;
        for function in &module.functions {
            let Some(identity) = &function.identity else {
                continue;
            };
            let mut steps = Vec::new();
            for instruction in &function.instructions {
                cancel
                    .check()
                    .map_err(|_| BytecodeVerificationError::Cancelled)?;
                match instruction {
                    BytecodeInstruction::MakeInterface {
                        module: owner,
                        implementation,
                        arguments,
                        ..
                    } => {
                        let table = &program.modules[owner.index()].interface_tables
                            [implementation.index()];
                        steps.push(ConstructionStep::Interface(ConcreteFunctionIdentity {
                            declaration: table.declaration,
                            arguments: arguments.clone(),
                        }));
                    }
                    BytecodeInstruction::Call {
                        callee: CallTarget::InterfaceMethod { contract, .. },
                        ..
                    } => {
                        steps.push(ConstructionStep::Evidence {
                            operations: contract.operations.clone(),
                            constructions: contract.constructions.clone(),
                        });
                        steps.push(ConstructionStep::Dispatch(contract.as_ref().clone()));
                    }
                    BytecodeInstruction::Call {
                        callee:
                            CallTarget::Shared {
                                module: owner,
                                target,
                                contract,
                            },
                        ..
                    } => {
                        steps.push(ConstructionStep::Evidence {
                            operations: contract.operations.clone(),
                            constructions: contract.constructions.clone(),
                        });
                        let owner = &program.modules[owner.index()];
                        let identity = match target {
                            CallableTarget::Script(target) => {
                                owner.functions[target.index()].identity.as_ref()
                            }
                            CallableTarget::Native(target) => {
                                Some(&owner.native_imports[target.index()].instance)
                            }
                        };
                        if let Some(identity) = identity {
                            steps.push(ConstructionStep::Call {
                                body: identity.clone(),
                                arguments: contract.arguments.clone(),
                            });
                        }
                    }
                    BytecodeInstruction::MakeClosure {
                        function: target, ..
                    } => {
                        let target = &module.functions[target.index()];
                        if let Some(identity) = &target.identity {
                            steps.push(ConstructionStep::Call {
                                body: identity.clone(),
                                arguments: target
                                    .metadata
                                    .semantic
                                    .generic
                                    .as_ref()
                                    .map_or_else(Vec::new, |body| {
                                        body.parameters.iter().map(|p| p.as_type()).collect()
                                    }),
                            });
                        }
                    }
                    _ => {}
                }
            }
            bodies.push(ConstructionBody {
                identity: identity.clone(),
                parameters: function
                    .metadata
                    .semantic
                    .generic
                    .as_ref()
                    .map_or_else(Vec::new, |body| body.parameters.clone()),
                steps,
            });
        }
        for import in &module.native_imports {
            if !bodies.iter().any(|body| body.identity == import.instance)
                && let Some(body) = native_construction(import)
            {
                bodies.push(body);
            }
        }
        for linked in &module.interface_tables {
            if !linked.arguments.iter().all(Ty::is_concrete) {
                continue;
            }
            let Some(template) = module.public_items.iter().find_map(|item| match item {
                PublicItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                    Some(table)
                }
                _ => None,
            }) else {
                continue;
            };
            let Some(table) = template.instantiate(&linked.arguments, &[], definitions) else {
                continue;
            };
            let mut bindings = TypeSubstitution::default();
            for (parameter, argument) in template.generic_params.iter().zip(&linked.arguments) {
                bindings.bind(&parameter.owner, parameter.position, argument);
            }
            let Ty::Trait(interface) = &table.trait_type else {
                continue;
            };
            let Some(contract) = program.modules.iter().find_map(|module| {
                trait_contract(
                    &module.identity,
                    &module.public_items,
                    &module.trait_contracts,
                    &interface.declaration,
                    definitions,
                )
            }) else {
                continue;
            };
            for slot in &linked.methods {
                let identity = match slot.target {
                    CallableTarget::Script(target) => {
                        module.functions[target.index()].identity.as_ref()
                    }
                    CallableTarget::Native(target) => {
                        Some(&module.native_imports[target.index()].instance)
                    }
                };
                let Some(identity) = identity else {
                    continue;
                };
                let Some(name) = definitions
                    .resolve(slot.method)
                    .ok()
                    .and_then(|view| view.last())
                    .map(|part| part.name)
                else {
                    continue;
                };
                let Some(method_slot) = contract
                    .methods
                    .iter()
                    .position(|method| method.name == name)
                else {
                    continue;
                };
                let Some(method) = table.methods.iter().find(|method| method.name == name) else {
                    continue;
                };
                let arguments = slot
                    .arguments
                    .iter()
                    .map(|ty| bindings.apply(ty, cancel).map_err(|_| invalid()))
                    .collect::<Result<Vec<_>, _>>()?;
                methods.push(ConstructionMethod {
                    interface: interface.clone(),
                    receiver: table.for_type.clone(),
                    slot: method_slot as u32,
                    body: identity.clone(),
                    parameters: method.generic_params.clone(),
                    arguments,
                });
            }
        }
    }
    let demands = closed_constructions(&bodies, &methods, MAX_ARTIFACT_TABLE_RECORDS, cancel)
        .map_err(|_| invalid())?;
    for demand in demands {
        let Some((owner, _)) = program.modules.iter().find_map(|module| {
            module.public_items.iter().find_map(|item| match item {
                PublicItem::InterfaceTable(table) if table.declaration == demand.declaration => {
                    table
                        .instantiate(&demand.arguments, &[], definitions)
                        .map(|table| (module, table))
                }
                _ => None,
            })
        }) else {
            return Err(invalid());
        };
        if !owner.interface_tables.iter().any(|table| {
            table.declaration == demand.declaration && table.arguments == demand.arguments
        }) {
            return Err(invalid());
        }
    }
    Ok(())
}
