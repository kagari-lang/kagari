//! Verified executable modules and concrete instance-to-module/function link bindings.
mod applications;
mod native;
use kagari_abi::{
    contracts, host,
    language::Protocol,
    representation::ValueType,
    types::{
        self as abi, AbiType, ConcreteFunctionIdentity, PublicAbiItem, inheritance,
        substitution::TypeTransformError,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    host_interface::HostInterface,
    identity::{DefinitionId, DefinitionKind, ModuleIdentity},
};
use std::collections::{HashMap, HashSet};

use crate::{
    function::MirModule,
    ids::InstanceId,
    instruction::{CallTarget, Instruction},
    verify::{MirVerificationError, VerificationBudget, VerifiedMirModule, verify_with_budget},
};

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
    UnresolvedFunction(DefinitionId),
    FunctionContract(DefinitionId),
    StructContract(DefinitionId),
    EnumContract(DefinitionId),
    InterfaceContract(DefinitionId),
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
}

impl VerifiedMirProgram {
    pub fn root(&self) -> &ModuleIdentity {
        &self.root
    }
    pub fn modules(&self) -> &[VerifiedMirModule] {
        &self.modules
    }
    pub fn function(&self, instance: &ConcreteFunctionIdentity) -> Option<ProgramFunctionRef> {
        self.bindings.get(instance).copied()
    }
    pub fn into_unverified(self) -> Vec<MirModule> {
        self.modules
            .into_iter()
            .map(VerifiedMirModule::into_unverified)
            .collect()
    }
}

/// Revalidates every module and every cross-module binding after any IR edit.
pub fn verify_program(
    root: ModuleIdentity,
    raw: Vec<MirModule>,
    cancel: &CancellationToken,
) -> Result<VerifiedMirProgram, ProgramError> {
    let error = |module: &ModuleIdentity, kind| ProgramError {
        module: Box::new(module.clone()),
        kind,
    };
    cancel
        .check()
        .map_err(|_| error(&root, ProgramErrorKind::Cancelled))?;
    let mut modules = Vec::with_capacity(raw.len());
    let mut indices = HashMap::new();
    let mut bindings = HashMap::new();
    let mut layouts = HashMap::new();
    let mut enum_layouts = HashMap::new();
    for (index, module) in raw.iter().enumerate() {
        cancel
            .check()
            .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?;
        if indices.insert(module.identity.clone(), index).is_some() {
            return Err(error(&module.identity, ProgramErrorKind::InvalidGraph));
        }
    }
    let mut budget = VerificationBudget::default();
    for (index, module) in raw.into_iter().enumerate() {
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
        let module = verify_with_budget(module, cancel, &mut budget)
            .map_err(|cause| error(&identity, ProgramErrorKind::Verification(cause)))?;
        for layout in &module.structures {
            cancel
                .check()
                .map_err(|_| error(&identity, ProgramErrorKind::Cancelled))?;
            if let Some(previous) = layouts.insert(
                (layout.declaration.clone(), layout.arguments.clone()),
                layout.clone(),
            ) && previous != *layout
            {
                return Err(error(
                    &identity,
                    ProgramErrorKind::StructContract(layout.declaration.clone()),
                ));
            }
        }
        for layout in &module.enumerations {
            cancel
                .check()
                .map_err(|_| error(&identity, ProgramErrorKind::Cancelled))?;
            if let Some(previous) = enum_layouts.insert(
                (layout.declaration.clone(), layout.arguments.clone()),
                layout.clone(),
            ) && previous != *layout
            {
                return Err(error(
                    &identity,
                    ProgramErrorKind::EnumContract(layout.declaration.clone()),
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
                    ProgramErrorKind::FunctionContract(instance.declaration),
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
        if !native::validate(module, &closure, cancel).map_err(|cause| {
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
        applications::validate(module, cancel, |id| {
            let owner = *indices.get(&id.module)?;
            if owner != index && !dependencies.contains(&owner) {
                return None;
            }
            let owner = &modules[owner];
            abi::trait_contract(
                &owner.identity,
                &owner.abi.public_items,
                &owner.abi.trait_contracts,
                id,
            )
        })
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
            let owner = indices.get(&id.module);
            if Protocol::from_id(id).is_some()
                && !owner.is_some_and(|owner| *owner == index || dependencies.contains(owner))
            {
                return Err(error(
                    &module.identity,
                    ProgramErrorKind::InterfaceContract(id.clone()),
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
                )
                .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?
                {
                    return Err(error(
                        &module.identity,
                        ProgramErrorKind::InterfaceContract(id.clone()),
                    ));
                }
            }
        }
        for request in &module.interface_instances {
            let valid = indices
                .get(&request.declaration.module)
                .filter(|owner| **owner == index || dependencies.contains(owner))
                .is_some_and(|owner| {
                    modules[*owner].abi.public_items.iter().any(|item| {
                        let PublicAbiItem::InterfaceTable(table) = item else {
                            return false;
                        };
                        table.declaration == request.declaration
                            && table.instantiate(&request.arguments).is_some()
                    })
                });
            if !valid {
                return Err(error(
                    &module.identity,
                    ProgramErrorKind::InterfaceContract(request.declaration.clone()),
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
        for instruction in module
            .functions
            .iter()
            .flat_map(|f| &f.blocks)
            .flat_map(|b| &b.instructions)
        {
            cancel
                .check()
                .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?;
            if let Instruction::UpcastInterface { source, target, .. } = instruction {
                let ancestry = inheritance::trait_closure(
                    source,
                    &AbiType::Trait(source.clone()),
                    cancel,
                    &|id| {
                        let owner = *indices.get(&id.module)?;
                        if owner != index && !dependencies.contains(&owner) {
                            return None;
                        }
                        let abi = &modules[owner].abi;
                        abi.trait_contracts
                            .iter()
                            .find(|record| record.declaration == *id)
                            .map(|record| &record.abi)
                            .or_else(|| {
                                abi.public_items.iter().find_map(|item| match item {
                                    PublicAbiItem::Trait(record)
                                        if id.path.len() == 1
                                            && id.path[0].name == record.name
                                            && id.path[0].kind == DefinitionKind::Trait
                                            && id.path[0].occurrence == 0 =>
                                    {
                                        Some(record)
                                    }
                                    _ => None,
                                })
                            })
                    },
                );
                if !ancestry.is_ok_and(|parents| parents.contains(target)) {
                    return Err(error(
                        &module.identity,
                        ProgramErrorKind::InterfaceContract(target.declaration.clone()),
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
                    .get(&contract.interface.declaration.module)
                    .filter(|target| **target == index || dependencies.contains(target))
                    .and_then(|target| {
                        let owner = &modules[*target];
                        abi::interface_method_types(
                            &owner.identity,
                            &owner.abi.public_items,
                            &owner.abi.trait_contracts,
                            &contract.interface,
                            contract.method_slot as usize,
                        )
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
                        ProgramErrorKind::InterfaceContract(contract.interface.declaration.clone()),
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
                    .get(&implementation.module)
                    .filter(|target| **target == index || dependencies.contains(target))
                    .and_then(|target| {
                        modules[*target].abi.public_items.iter().find_map(|item| {
                            if let PublicAbiItem::InterfaceTable(table) = item
                                && table.declaration == *implementation
                            {
                                table.instantiate(arguments)
                            } else {
                                None
                            }
                        })
                    })
                    .is_some_and(|table| {
                        dst.ty == ValueType::HeapObject
                            && value.ty == table.for_type.representation()
                            && table.generic_params.is_empty()
                            && table.for_type.is_concrete()
                            && table.trait_type.is_concrete()
                            && table
                                .methods
                                .iter()
                                .all(|method| method.generic_params.is_empty())
                    });
                if !valid {
                    return Err(error(
                        &module.identity,
                        ProgramErrorKind::InterfaceContract(implementation.clone()),
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
                declaration: contract.declaration.clone(),
                arguments: contract.arguments.clone(),
            };
            let target = bindings.get(&key).ok_or_else(|| {
                error(
                    &module.identity,
                    ProgramErrorKind::UnresolvedFunction(contract.declaration.clone()),
                )
            })?;
            if target.module == index || !dependencies.contains(&target.module) {
                return Err(error(
                    &module.identity,
                    ProgramErrorKind::UnresolvedFunction(contract.declaration.clone()),
                ));
            }
            let function = &modules[target.module].functions[target.function.index()];
            if function
                .params
                .iter()
                .map(|p| p.ty)
                .ne(contract.params.iter().copied())
                || function.return_type != contract.return_type
            {
                return Err(error(
                    &module.identity,
                    ProgramErrorKind::FunctionContract(contract.declaration.clone()),
                ));
            }
        }
    }
    Ok(VerifiedMirProgram {
        root,
        modules,
        bindings,
    })
}
