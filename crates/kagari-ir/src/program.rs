//! Verified source modules and declaration-to-module/function link bindings.
use std::collections::{HashMap, HashSet};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, ModuleIdentity},
};
use kagari_hir::program::CheckedProgram;

use crate::{
    IrLoweringError, IrLoweringOptions, lower_to_ir,
    module::{
        CallTarget, Instruction, IrModule, IrVerificationError, VerifiedIrModule, ids::InstanceId,
        verify_ir,
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
    bindings: HashMap<DefinitionId, ProgramFunctionRef>,
}

impl VerifiedIrProgram {
    pub fn root(&self) -> &ModuleIdentity {
        &self.root
    }
    pub fn modules(&self) -> &[VerifiedIrModule] {
        &self.modules
    }
    pub fn function(&self, declaration: &DefinitionId) -> Option<ProgramFunctionRef> {
        self.bindings.get(declaration).copied()
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
    let mut modules = Vec::new();
    let mut remaining = options.clone();
    for module in program.modules() {
        let lowered = lower_to_ir(module, &remaining).map_err(|mut error| {
            if let IrLoweringError::Diagnostic(diagnostic) = &mut error
                && let kagari_common::DiagnosticKind::CompileLimitExceeded { resource, limit } =
                    &mut diagnostic.kind
            {
                match *resource {
                    "generated instructions" => *limit = options.max_instructions,
                    "generic instances" => *limit = options.max_generic_instances,
                    _ => {}
                }
            }
            ProgramError {
                module: Box::new(module.lowered.source.module_identity().clone()),
                kind: ProgramErrorKind::Lowering(error),
            }
        })?;
        // These budgets apply to the whole source closure, not once per module.
        remaining.max_generic_instances -= lowered
            .functions
            .iter()
            .filter(|f| !f.instance.arguments.is_empty())
            .count();
        remaining.max_instructions -= lowered
            .functions
            .iter()
            .flat_map(|f| &f.blocks)
            .map(|b| b.instructions.len() + usize::from(b.terminator.is_some()))
            .sum::<usize>();
        modules.push(lowered.into_unverified());
    }
    verify_program(root, modules, &options.cancel)
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
    for (index, module) in raw.into_iter().enumerate() {
        cancel
            .check()
            .map_err(|_| error(&module.identity, ProgramErrorKind::Cancelled))?;
        if indices.contains_key(&module.identity)
            || module
                .dependencies
                .iter()
                .any(|dependency| !indices.contains_key(dependency))
        {
            return Err(error(&module.identity, ProgramErrorKind::InvalidGraph));
        }
        indices.insert(module.identity.clone(), index);
        let identity = module.identity.clone();
        let module = verify_ir(module, cancel)
            .map_err(|cause| error(&identity, ProgramErrorKind::Verification(cause)))?;
        for layout in &module.structures {
            cancel
                .check()
                .map_err(|_| error(&identity, ProgramErrorKind::Cancelled))?;
            if let Some(previous) = layouts.insert(layout.declaration.clone(), layout.clone())
                && previous != *layout
            {
                return Err(error(
                    &identity,
                    ProgramErrorKind::StructContract(layout.declaration.clone()),
                ));
            }
        }
        for function in &module.functions {
            if function.instance.arguments.is_empty() {
                let declaration = function.instance.declaration.clone();
                if bindings
                    .insert(
                        declaration.clone(),
                        ProgramFunctionRef {
                            module: index,
                            function: function.id,
                        },
                    )
                    .is_some()
                {
                    return Err(error(
                        &identity,
                        ProgramErrorKind::FunctionContract(declaration),
                    ));
                }
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
            let Instruction::Call {
                callee: CallTarget::SourceFunction(contract),
                ..
            } = instruction
            else {
                continue;
            };
            let target = bindings.get(&contract.declaration).ok_or_else(|| {
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
