//! Verified executable modules and concrete instance-to-module/function link bindings.
use kagari_contract::representation::semantic_representation;
use kagari_types::declaration::TypeDefKind;
mod applications;
mod native;
mod shared;
use crate::{
    function::MirModule,
    ids::InstanceId,
    instruction::{CallTarget, Instruction},
    verify::{
        MirVerificationError, ValidatedMirModule, VerificationBudget, VerifiedMirModule, ownership,
        verify_with_budget,
    },
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
        table::DefinitionTable,
    },
};
use kagari_contract::{
    contracts, host, types as abi,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_types::{
    host_interface::HostInterface,
    language::Protocol,
    ty::{Ty, inheritance, substitution::TypeTransformError},
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramFunctionRef {
    pub module: usize,
    pub function: InstanceId,
}

#[derive(Debug)]
pub enum ProgramErrorKind {
    Cancelled,
    InvalidGraph,
    Verification(MirVerificationError),
    UnresolvedFunction(DefinitionPath),
    FunctionContract(DefinitionPath),
    StructContract(DefinitionPath),
    EnumContract(DefinitionPath),
    InterfaceContract(DefinitionPath),
}

#[derive(Debug)]
pub struct ProgramError {
    pub module: Box<ModuleIdentity>,
    pub kind: ProgramErrorKind,
}

#[derive(Debug, Clone)]
pub struct VerifiedMirProgram {
    root: ModuleIdentity,
    modules: Vec<VerifiedMirModule>,
    bindings: HashMap<ConcreteFunctionIdentity, ProgramFunctionRef>,
    definitions: DefinitionTable,
}

impl VerifiedMirProgram {
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    pub fn root(&self) -> &ModuleIdentity {
        &self.root
    }

    pub fn modules(&self) -> &[VerifiedMirModule] {
        &self.modules
    }

    pub fn function(&self, instance: &ConcreteFunctionIdentity) -> Option<ProgramFunctionRef> {
        self.bindings.get(instance).copied()
    }

    /// Discard program and module seals while retaining their shared identity scope.
    pub fn into_unverified(self) -> DefinitionMetadata<Vec<MirModule>> {
        DefinitionMetadata::checked(
            self.definitions,
            self.modules
                .into_iter()
                .map(|module| module.into_unverified().into_records())
                .collect(),
            &CancellationToken::default(),
        )
        .expect("verified program references remain in their retained scope")
    }
}

/// Revalidates every module and every cross-module binding after any IR edit.
/// All input references must belong to the supplied context's current prefix;
/// import foreign metadata explicitly before calling. ABI-owned names are then
/// collected without granting declaration or execution authority. Success retains
/// an immutable prefix containing the checked graph and its proof identities.
pub fn verify_program(
    root: ModuleIdentity,
    inputs: Vec<MirModule>,
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<VerifiedMirProgram, ProgramError> {
    let error = |module: &ModuleIdentity, kind| ProgramError {
        module: Box::new(module.clone()),
        kind,
    };
    cancel
        .check()
        .map_err(|_| error(&root, ProgramErrorKind::Cancelled))?;
    let definitions = ownership::collect(&inputs, context, cancel).map_err(|cause| {
        error(
            &root,
            ProgramErrorKind::Verification(ownership::mapping_error(cause)),
        )
    })?;
    let path = |id| {
        definitions
            .resolve(id)
            .expect("reference-validated MIR input")
            .to_path()
    };
    let mut modules = Vec::with_capacity(inputs.len());
    let mut indices = HashMap::new();
    let mut bindings = HashMap::new();
    let mut layouts = HashMap::new();
    let mut enum_layouts = HashMap::new();
    for (index, module) in inputs.iter().enumerate() {
        cancel
            .check()
            .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?;
        if indices.insert(module.identity.clone(), index).is_some() {
            return Err(error(&module.identity, ProgramErrorKind::InvalidGraph));
        }
    }
    let mut budget = VerificationBudget::default();
    for (index, module) in inputs.into_iter().enumerate() {
        cancel
            .check()
            .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?;
        if module
            .dependencies
            .iter()
            .any(|dependency| !indices.contains_key(dependency))
        {
            return Err(error(&module.identity, ProgramErrorKind::InvalidGraph));
        }
        let identity = module.identity.clone();
        let metadata =
            DefinitionMetadata::checked(definitions.clone(), module, cancel).map_err(|cause| {
                error(
                    &identity,
                    ProgramErrorKind::Verification(ownership::mapping_error(cause)),
                )
            })?;
        let module = verify_with_budget(metadata, cancel, &mut budget)
            .map_err(|cause| error(&identity, ProgramErrorKind::Verification(cause)))?;
        for layout in &module.structures {
            cancel
                .check()
                .map_err(|_| error(&identity, ProgramErrorKind::Cancelled))?;
            if let Some(previous) = layouts.insert(
                (layout.declaration, layout.arguments.clone()),
                layout.clone(),
            ) && previous != *layout
            {
                return Err(error(
                    &identity,
                    ProgramErrorKind::StructContract(path(layout.declaration)),
                ));
            }
        }
        for layout in &module.enumerations {
            cancel
                .check()
                .map_err(|_| error(&identity, ProgramErrorKind::Cancelled))?;
            if let Some(previous) = enum_layouts.insert(
                (layout.declaration, layout.arguments.clone()),
                layout.clone(),
            ) && previous != *layout
            {
                return Err(error(
                    &identity,
                    ProgramErrorKind::EnumContract(path(layout.declaration)),
                ));
            }
        }
        for function in &module.functions {
            let instance = function.instance.clone();
            if bindings
                .insert(
                    instance.clone(),
                    ProgramFunctionRef {
                        module: index,
                        function: function.id,
                    },
                )
                .is_some()
            {
                return Err(error(
                    &identity,
                    ProgramErrorKind::FunctionContract(path(instance.declaration)),
                ));
            }
        }
        modules.push(module);
    }
    let Some(root_index) = indices.get(&root).copied() else {
        return Err(error(&root, ProgramErrorKind::InvalidGraph));
    };
    let mut reachable = HashSet::new();
    let mut pending = vec![root_index];
    while let Some(index) = pending.pop() {
        cancel
            .check()
            .map_err(|_| error(&root, ProgramErrorKind::Cancelled))?;
        if reachable.insert(index) {
            pending.extend(modules[index].dependencies.iter().map(|id| indices[id]));
        }
    }
    if reachable.len() != modules.len() {
        return Err(error(&root, ProgramErrorKind::InvalidGraph));
    }
    // Calls may follow public facades, so their owner can be a transitive dependency.
    for (index, module) in modules.iter().enumerate() {
        let mut dependencies = HashSet::new();
        let mut pending = module
            .dependencies
            .iter()
            .map(|id| indices[id])
            .collect::<Vec<_>>();
        while let Some(dependency) = pending.pop() {
            cancel
                .check()
                .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?;
            if dependencies.insert(dependency) {
                pending.extend(
                    modules[dependency]
                        .dependencies
                        .iter()
                        .map(|id| indices[id]),
                );
            }
        }
        let closure: Vec<_> = modules
            .iter()
            .enumerate()
            .filter_map(|(owner, module)| {
                (owner == index || dependencies.contains(&owner)).then_some(module)
            })
            .collect();
        applications::validate(
            module,
            cancel,
            |id| {
                let owner = *indices.get(definitions.resolve(*id).ok()?.module())?;
                if owner != index && !dependencies.contains(&owner) {
                    return None;
                }
                let owner = &modules[owner];
                abi::trait_contract(
                    &owner.identity,
                    &owner.abi.public_items,
                    &owner.abi.trait_contracts,
                    id,
                    &definitions,
                )
            },
            |id| {
                closure.iter().find_map(|owner| {
                    abi::type_contract(&owner.identity, &owner.abi.public_items, id, &definitions)
                        .map(|record| (record.kind, record.generic_params.len()))
                        .or_else(|| {
                            owner
                                .enumerations
                                .iter()
                                .find(|layout| &layout.declaration == id)
                                .map(|layout| (TypeDefKind::Enum, layout.arguments.len()))
                        })
                        .or_else(|| {
                            owner
                                .structures
                                .iter()
                                .find(|layout| &layout.declaration == id)
                                .map(|layout| (TypeDefKind::Struct, layout.arguments.len()))
                        })
                })
            },
        )
        .map_err(|cause| {
            error(
                &module.identity,
                match cause {
                    TypeTransformError::Cancelled => ProgramErrorKind::Cancelled,
                    _ => ProgramErrorKind::InvalidGraph,
                },
            )
        })?;
        let host_interface = HostInterface {
            types: module.host_types.clone(),
            ..Default::default()
        };
        for implementation in host_interface
            .types
            .iter()
            .flat_map(|host| &host.trait_implementations)
        {
            let id = &implementation.trait_id;
            let owner = indices.get(
                definitions
                    .resolve(*id)
                    .map_err(|_| error(&module.identity, ProgramErrorKind::InvalidGraph))?
                    .module(),
            );
            if Protocol::from_reference(id, Some(&definitions)).is_some()
                && !owner.is_some_and(|owner| *owner == index || dependencies.contains(owner))
            {
                return Err(error(
                    &module.identity,
                    ProgramErrorKind::InterfaceContract(path(*id)),
                ));
            }
            if let Some(owner) = owner {
                let owner = &modules[*owner];
                if !host::trait_bindings_match(
                    &host_interface,
                    &owner.identity,
                    &owner.abi.public_items,
                    &owner.abi.trait_contracts,
                    cancel,
                    &definitions,
                )
                .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?
                {
                    return Err(error(
                        &module.identity,
                        ProgramErrorKind::InterfaceContract(path(*id)),
                    ));
                }
            }
        }
        for request in &module.interface_instances {
            let valid = indices
                .get(
                    definitions
                        .resolve(request.declaration)
                        .map_err(|_| error(&module.identity, ProgramErrorKind::InvalidGraph))?
                        .module(),
                )
                .filter(|owner| **owner == index || dependencies.contains(owner))
                .is_some_and(|owner| {
                    modules[*owner].abi.public_items.iter().any(|item| {
                        let PublicItem::InterfaceTable(table) = item else {
                            return false;
                        };
                        table.declaration == request.declaration
                            && table
                                .instantiate(
                                    &request.arguments,
                                    &table.generic_params,
                                    &definitions,
                                )
                                .is_some()
                    })
                });
            if !valid {
                return Err(error(
                    &module.identity,
                    ProgramErrorKind::InterfaceContract(path(request.declaration)),
                ));
            }
        }
        for function in &module.functions {
            if function.debug.source_module.as_ref().is_some_and(|origin| {
                !indices
                    .get(origin)
                    .is_some_and(|owner| *owner == index || dependencies.contains(owner))
            }) {
                return Err(error(&module.identity, ProgramErrorKind::InvalidGraph));
            }
        }
        for (function, instruction) in module.functions.iter().flat_map(|function| {
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .map(move |instruction| (function, instruction))
        }) {
            cancel
                .check()
                .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?;
            if let Instruction::UpcastInterface { source, target, .. } = instruction {
                let ancestry =
                    inheritance::trait_closure(source, &Ty::Trait(source.clone()), cancel, &|id| {
                        let owner = *indices.get(definitions.resolve(*id).ok()?.module())?;
                        if owner != index && !dependencies.contains(&owner) {
                            return None;
                        }
                        let abi = &modules[owner].abi;
                        abi::trait_contract(
                            &modules[owner].identity,
                            &abi.public_items,
                            &abi.trait_contracts,
                            id,
                            &definitions,
                        )
                    });
                if !ancestry.is_ok_and(|parents| parents.contains(target)) {
                    return Err(error(
                        &module.identity,
                        ProgramErrorKind::InterfaceContract(path(target.declaration)),
                    ));
                }
                continue;
            }
            if let Instruction::Call {
                dst,
                callee: CallTarget::InterfaceMethod(contract),
                args,
            } = instruction
            {
                let valid = indices
                    .get(
                        definitions
                            .resolve(contract.interface.declaration)
                            .map_err(|_| error(&module.identity, ProgramErrorKind::InvalidGraph))?
                            .module(),
                    )
                    .filter(|target| **target == index || dependencies.contains(target))
                    .and_then(|target| {
                        let owner = &modules[*target];
                        contract
                            .signature_in(
                                &owner.identity,
                                &owner.abi.public_items,
                                &owner.abi.trait_contracts,
                                &Default::default(),
                                &definitions,
                            )
                            .ok()
                            .and_then(|signature| {
                                signature.physical_types(
                                    function
                                        .semantic
                                        .generic
                                        .as_ref()
                                        .map_or(&[], |body| body.parameters.as_slice()),
                                    &Default::default(),
                                    &definitions,
                                )
                            })
                    })
                    .is_some_and(|(params, return_type)| {
                        args.len() == params.len()
                            && args.iter().zip(params).all(|(arg, param)| arg.ty == param)
                            && contracts::verify_call_dst(dst.map(|value| value.ty), return_type)
                                .is_ok()
                    });
                if !valid {
                    return Err(error(
                        &module.identity,
                        ProgramErrorKind::InterfaceContract(path(contract.interface.declaration)),
                    ));
                }
                continue;
            }
            if let Instruction::MakeInterface {
                dst,
                value,
                implementation,
                arguments,
            } = instruction
            {
                let valid = indices
                    .get(
                        definitions
                            .resolve(*implementation)
                            .map_err(|_| error(&module.identity, ProgramErrorKind::InvalidGraph))?
                            .module(),
                    )
                    .filter(|target| **target == index || dependencies.contains(target))
                    .and_then(|target| {
                        modules[*target].abi.public_items.iter().find_map(|item| {
                            if let PublicItem::InterfaceTable(table) = item
                                && table.declaration == *implementation
                            {
                                table.instantiate(
                                    arguments,
                                    function
                                        .semantic
                                        .generic
                                        .as_ref()
                                        .map_or(&[], |body| &body.parameters),
                                    &definitions,
                                )
                            } else {
                                None
                            }
                        })
                    })
                    .is_some_and(|table| {
                        dst.ty == ValueType::HeapObject
                            && value.ty == semantic_representation(&table.for_type)
                            && table.generic_params.is_empty()
                    });
                if !valid {
                    return Err(error(
                        &module.identity,
                        ProgramErrorKind::InterfaceContract(path(*implementation)),
                    ));
                }
                continue;
            }
            let Instruction::Call {
                callee: CallTarget::SourceFunction(contract),
                ..
            } = instruction
            else {
                continue;
            };
            let key = ConcreteFunctionIdentity {
                declaration: contract.declaration,
                arguments: contract.arguments.clone(),
            };
            let target = bindings.get(&key).ok_or_else(|| {
                error(
                    &module.identity,
                    ProgramErrorKind::UnresolvedFunction(path(contract.declaration)),
                )
            })?;
            if target.module == index || !dependencies.contains(&target.module) {
                return Err(error(
                    &module.identity,
                    ProgramErrorKind::UnresolvedFunction(path(contract.declaration)),
                ));
            }
            let function = &modules[target.module].functions[target.function.index()];
            if function
                .params
                .iter()
                .map(|p| p.ty)
                .ne(contract.params.iter().copied())
                || function.return_type != contract.return_type
                || function.effects.may_suspend
            {
                return Err(error(
                    &module.identity,
                    ProgramErrorKind::FunctionContract(path(contract.declaration)),
                ));
            }
        }
        if !native::validate(module, &closure, &definitions, cancel).map_err(|cause| {
            error(
                &module.identity,
                match cause {
                    TypeTransformError::Cancelled => ProgramErrorKind::Cancelled,
                    _ => ProgramErrorKind::InvalidGraph,
                },
            )
        })? {
            return Err(error(&module.identity, ProgramErrorKind::InvalidGraph));
        }
    }
    cancel
        .check()
        .map_err(|_| error(&root, ProgramErrorKind::Cancelled))?;
    let modules = modules.into_iter().map(ValidatedMirModule::seal).collect();
    Ok(VerifiedMirProgram {
        root,
        modules,
        bindings,
        definitions,
    })
}
