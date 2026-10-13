//! Extract shared construction demands from MIR's checked callable structure.
use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_contract::{
    callable::construction::{
        ConstructionBody, ConstructionMethod, ConstructionStep, closed_constructions,
        native_construction,
    },
    decode_limits::MAX_TABLE_RECORDS,
    types::{ConcreteFunctionIdentity, PublicItem, trait_contract},
};
use kagari_types::{
    callable::CallableImplementation,
    ty::{GenericParam, Ty, substitution::TypeTransformError},
};
use std::collections::HashSet;

/// Enumerate concrete interface applications reached by closed shared calls.
/// The source planner materializes these tables; shared executable bodies remain
/// shared. The graph algorithm is also used by bytecode's independent validation.
pub fn closed_interface_instances(
    modules: &[&MirModule],
    definitions: &DefinitionTable,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<ConcreteFunctionIdentity>, TypeTransformError> {
    let mut bodies = Vec::new();
    for module in modules {
        for function in &module.functions {
            let mut steps = Vec::new();
            for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
                match instruction {
                    Instruction::MakeInterface {
                        implementation,
                        arguments,
                        ..
                    } => {
                        steps.push(ConstructionStep::Interface(ConcreteFunctionIdentity {
                            declaration: *implementation,
                            arguments: arguments.clone(),
                        }));
                    }
                    Instruction::Call {
                        callee: CallTarget::InterfaceMethod(call),
                        ..
                    } => {
                        steps.push(ConstructionStep::Dispatch(call.as_ref().clone()));
                    }
                    Instruction::Call {
                        callee: CallTarget::Shared(call),
                        ..
                    } => {
                        steps.push(ConstructionStep::Call {
                            body: call.instance.clone(),
                            arguments: call.arguments.clone(),
                        });
                    }
                    Instruction::MakeClosure {
                        function: target, ..
                    } => {
                        if let Some(target) = module.functions.get(target.index()) {
                            steps.push(ConstructionStep::Call {
                                body: target.instance.clone(),
                                arguments: target
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
                identity: function.instance.clone(),
                parameters: function
                    .semantic
                    .generic
                    .as_ref()
                    .map_or_else(Vec::new, |body| body.parameters.clone()),
                steps,
            });
        }
    }
    for import in modules
        .iter()
        .flat_map(|module| module.native_applications())
    {
        if !bodies.iter().any(|body| body.identity == import.instance)
            && let Some(body) = native_construction(import)
        {
            bodies.push(body);
        }
    }
    let mut methods = Vec::new();
    let mut seen = HashSet::new();
    for owner in modules {
        for instance in &owner.interface_instances {
            if !instance.arguments.iter().all(Ty::is_concrete) || !seen.insert(instance.clone()) {
                continue;
            }
            let Some((module, table)) = modules.iter().find_map(|module| {
                module.abi.public_items.iter().find_map(|item| match item {
                    PublicItem::InterfaceTable(table)
                        if table.declaration == instance.declaration =>
                    {
                        Some((module, table))
                    }
                    _ => None,
                })
            }) else {
                continue;
            };
            let Some(table) = table.instantiate(&instance.arguments, &[], definitions) else {
                continue;
            };
            let Ty::Trait(interface) = &table.trait_type else {
                continue;
            };
            let Some(contract) = modules.iter().find_map(|module| {
                trait_contract(
                    &module.identity,
                    &module.abi.public_items,
                    &module.abi.trait_contracts,
                    &interface.declaration,
                    definitions,
                )
            }) else {
                continue;
            };
            for method in &table.methods {
                let Some(slot) = contract
                    .methods
                    .iter()
                    .position(|required| required.name == method.name)
                else {
                    continue;
                };
                let Some(declaration) = definitions.lookup_child(
                    instance.declaration,
                    DefinitionKind::Method,
                    &method.name,
                    0,
                ) else {
                    continue;
                };
                let selected = match &method.implementation {
                    CallableImplementation::NativeDefault(application) => module
                        .native_targets
                        .iter()
                        .find(|target| target.instance.declaration == application.declaration)
                        .map(|target| (target.instance.clone(), application.arguments.clone())),
                    CallableImplementation::Native(_) => {
                        let shared_receiver = !instance.arguments.is_empty()
                            && method.bounds.iter().any(|bound| {
                                bound.constraints.iter().any(|constraint| {
                                    !table.bounds.iter().any(|assumed| {
                                        assumed.ty == bound.ty
                                            && assumed.constraints.contains(constraint)
                                    })
                                })
                            });
                        let arguments: Vec<_> = instance
                            .arguments
                            .iter()
                            .cloned()
                            .chain(method.generic_params.iter().map(|p| p.as_type()))
                            .collect();
                        let identity = ConcreteFunctionIdentity {
                            declaration,
                            arguments: if shared_receiver {
                                (0..arguments.len())
                                    .map(|position| {
                                        GenericParam {
                                            owner: declaration,
                                            position,
                                        }
                                        .as_type()
                                    })
                                    .collect()
                            } else {
                                arguments.clone()
                            },
                        };
                        module
                            .native_targets
                            .iter()
                            .find(|target| target.instance == identity)
                            .map(|target| {
                                let entry = if shared_receiver {
                                    arguments
                                } else {
                                    method.generic_params.iter().map(|p| p.as_type()).collect()
                                };
                                (target.instance.clone(), entry)
                            })
                    }
                    _ => {
                        let target = module
                            .functions
                            .iter()
                            .find(|function| {
                                function.instance.declaration == declaration
                                    && function.instance.arguments == instance.arguments
                            })
                            .or_else(|| {
                                module.functions.iter().find(|function| {
                                    function.instance.declaration == declaration
                                        && function.instance.arguments.is_empty()
                                })
                            });
                        target.and_then(|target| {
                            let parameter_count = target
                                .semantic
                                .generic
                                .as_ref()
                                .map_or(0, |body| body.parameters.len());
                            let mut arguments = if parameter_count == method.generic_params.len() {
                                Vec::new()
                            } else if parameter_count
                                == instance.arguments.len() + method.generic_params.len()
                            {
                                instance.arguments.clone()
                            } else {
                                return None;
                            };
                            arguments.extend(method.generic_params.iter().map(|p| p.as_type()));
                            Some((target.instance.clone(), arguments))
                        })
                    }
                };
                let Some((body, arguments)) = selected else {
                    continue;
                };
                methods.push(ConstructionMethod {
                    interface: interface.clone(),
                    receiver: table.for_type.clone(),
                    slot: slot as u32,
                    body,
                    parameters: method.generic_params.clone(),
                    arguments,
                });
            }
        }
    }
    closed_constructions(&bodies, &methods, limit, cancel)
}

pub(super) fn verify_closed_interfaces(
    modules: &[&MirModule],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), TypeTransformError> {
    for demand in closed_interface_instances(modules, definitions, MAX_TABLE_RECORDS, cancel)? {
        let Some((owner, _)) = modules.iter().find_map(|module| {
            module.abi.public_items.iter().find_map(|item| match item {
                PublicItem::InterfaceTable(table) if table.declaration == demand.declaration => {
                    table
                        .instantiate(&demand.arguments, &[], definitions)
                        .map(|table| (module, table))
                }
                _ => None,
            })
        }) else {
            return Err(TypeTransformError::InvalidContract);
        };
        if !owner.interface_instances.contains(&demand) {
            return Err(TypeTransformError::InvalidContract);
        }
    }
    Ok(())
}
