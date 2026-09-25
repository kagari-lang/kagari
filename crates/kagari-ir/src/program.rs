//! Verified source modules and concrete instance-to-module/function link bindings.
use std::collections::{HashMap, HashSet};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, ModuleIdentity},
};
use kagari_hir::program::CheckedProgram;

use crate::{
    IrLoweringError, IrLoweringOptions,
    module::{
        CallTarget, Instruction, IrModule, IrVerificationError, VerifiedIrModule,
        function::FunctionInstance, ids::InstanceId, verify_ir,
    },
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
    Lowering(IrLoweringError),
    Verification(IrVerificationError),
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
pub struct VerifiedIrProgram {
    root: ModuleIdentity,
    modules: Vec<VerifiedIrModule>,
    bindings: HashMap<FunctionInstance, ProgramFunctionRef>,
}

impl VerifiedIrProgram {
    pub fn root(&self) -> &ModuleIdentity {
        &self.root
    }
    pub fn modules(&self) -> &[VerifiedIrModule] {
        &self.modules
    }
    pub fn function(&self, instance: &FunctionInstance) -> Option<ProgramFunctionRef> {
        self.bindings.get(instance).copied()
    }
    pub fn into_unverified(self) -> Vec<IrModule> {
        self.modules
            .into_iter()
            .map(VerifiedIrModule::into_unverified)
            .collect()
    }
}

pub fn lower_program_to_ir(
    program: &CheckedProgram,
    options: &IrLoweringOptions,
) -> Result<VerifiedIrProgram, ProgramError> {
    let root = program.root().lowered.source.module_identity().clone();
    let mut requests: HashMap<ModuleIdentity, Vec<FunctionInstance>> = HashMap::new();
    let mut seen = HashSet::new();
    loop {
        options.cancel.check().map_err(|_| ProgramError {
            module: Box::new(root.clone()),
            kind: ProgramErrorKind::Cancelled,
        })?;
        let mut modules = Vec::new();
        let mut remaining = options.clone();
        for module in program.modules() {
            let identity = module.lowered.source.module_identity();
            let demanded = requests
                .get(identity)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let lowered = crate::lower::lower_to_ir_with_requests(module, &remaining, demanded)
                .map_err(|mut error| {
                    if let IrLoweringError::Diagnostic(diagnostic) = &mut error
                        && let kagari_common::DiagnosticKind::CompileLimitExceeded {
                            resource,
                            limit,
                        } = &mut diagnostic.kind
                    {
                        match *resource {
                            "generated instructions" => *limit = options.max_instructions,
                            "generic instances" => *limit = options.max_generic_instances,
                            _ => {}
                        }
                    }
                    ProgramError {
                        module: Box::new(identity.clone()),
                        kind: ProgramErrorKind::Lowering(error),
                    }
                })?;
            // These budgets apply to the whole source closure, not once per module.
            remaining.max_generic_instances -= lowered
                .functions
                .iter()
                .filter(|f| !f.instance.arguments.is_empty())
                .count()
                + lowered
                    .structures
                    .iter()
                    .filter(|s| !s.arguments.is_empty())
                    .count()
                + lowered
                    .enumerations
                    .iter()
                    .filter(|e| !e.arguments.is_empty())
                    .count();
            remaining.max_instructions -= lowered
                .functions
                .iter()
                .flat_map(|f| &f.blocks)
                .map(|b| b.instructions.len() + usize::from(b.terminator.is_some()))
                .sum::<usize>();
            modules.push(lowered.into_unverified());
        }
        let mut changed = false;
        for contract in modules
            .iter()
            .flat_map(|module| &module.functions)
            .flat_map(|function| &function.blocks)
            .flat_map(|block| &block.instructions)
            .filter_map(|instruction| match instruction {
                Instruction::Call {
                    callee: CallTarget::SourceFunction(contract),
                    ..
                } if !contract.arguments.is_empty() => Some(contract),
                _ => None,
            })
        {
            options.cancel.check().map_err(|_| ProgramError {
                module: Box::new(root.clone()),
                kind: ProgramErrorKind::Cancelled,
            })?;
            let instance = FunctionInstance {
                declaration: contract.declaration.clone(),
                arguments: contract.arguments.clone(),
            };
            if seen.insert(instance.clone()) {
                requests
                    .entry(instance.declaration.module.clone())
                    .or_default()
                    .push(instance);
                changed = true;
            }
        }
        if !changed {
            return verify_program(root, modules, &options.cancel);
        }
    }
}

/// Revalidates every module and every cross-module binding after any IR edit.
pub fn verify_program(
    root: ModuleIdentity,
    raw: Vec<IrModule>,
    cancel: &CancellationToken,
) -> Result<VerifiedIrProgram, ProgramError> {
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
        let module = verify_ir(module, cancel)
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
        for instruction in module
            .functions
            .iter()
            .flat_map(|f| &f.blocks)
            .flat_map(|b| &b.instructions)
        {
            cancel
                .check()
                .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?;
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
                        crate::module::abi::interface_method_types(
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
                            && crate::module::contracts::verify_call_dst(
                                dst.map(|value| value.ty),
                                return_type,
                            )
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
            } = instruction
            {
                let valid = indices
                    .get(&implementation.module)
                    .filter(|target| **target == index || dependencies.contains(target))
                    .and_then(|target| {
                        modules[*target].abi.public_items.iter().find_map(|item| {
                            if let crate::module::PublicAbiItem::InterfaceTable(table) = item
                                && table.declaration == *implementation
                            {
                                Some(table)
                            } else {
                                None
                            }
                        })
                    })
                    .is_some_and(|table| {
                        dst.ty == crate::module::ValueType::HeapObject
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
            let key = FunctionInstance {
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
    Ok(VerifiedIrProgram {
        root,
        modules,
        bindings,
    })
}
