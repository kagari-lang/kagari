//! Resolve construction dispatch against current borrowed output inventories.

use crate::function::{MirFunction, MirModule};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, table::DefinitionTable},
};
use kagari_contract::{
    callable::construction::ConstructionMethod,
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, ModuleContract, PublicItem, trait_contract},
};
use kagari_types::{
    callable::CallableImplementation,
    ty::{GenericParam, Ty, substitution::TypeTransformError},
};
use std::collections::HashSet;

/// A module view also usable before bridge wrappers receive candidate-local IDs.
pub struct ConstructionModule<'a> {
    pub identity: &'a ModuleIdentity,
    pub functions: &'a [MirFunction],
    pub bridge_functions: &'a [MirFunction],
    pub native_targets: &'a [NativeImport],
    pub interface_instances: &'a [ConcreteFunctionIdentity],
    pub abi: &'a ModuleContract,
}

impl<'a> From<&'a MirModule> for ConstructionModule<'a> {
    fn from(module: &'a MirModule) -> Self {
        Self {
            identity: &module.identity,
            functions: &module.functions,
            bridge_functions: &[],
            native_targets: &module.native_targets,
            interface_instances: &module.interface_instances,
            abi: &module.abi,
        }
    }
}

impl<'a> ConstructionModule<'a> {
    fn functions(&self) -> impl Iterator<Item = &'a MirFunction> {
        self.functions.iter().chain(self.bridge_functions)
    }
}

/// Refresh slot selection when bodies, native targets or interface tables grow.
/// Missing products are not cached as resolved; independent verification checks
/// the final closed graph using the same selection rules.
pub fn construction_methods(
    modules: &[ConstructionModule<'_>],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<Vec<ConstructionMethod>, TypeTransformError> {
    let mut methods = Vec::new();
    let mut seen = HashSet::new();
    for owner in modules {
        for instance in owner.interface_instances {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
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
                    module.identity,
                    &module.abi.public_items,
                    &module.abi.trait_contracts,
                    &interface.declaration,
                    definitions,
                )
            }) else {
                continue;
            };
            for method in &table.methods {
                cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
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
                        .map(|target| {
                            (
                                target.instance.clone(),
                                if target.generic.is_some() {
                                    application.arguments.clone()
                                } else {
                                    vec![]
                                },
                            )
                        }),
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
                            .functions()
                            .find(|function| {
                                function.instance.declaration == declaration
                                    && function.instance.arguments == instance.arguments
                            })
                            .or_else(|| {
                                module.functions().find(|function| {
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
    Ok(methods)
}
