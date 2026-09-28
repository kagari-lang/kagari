use crate::{MirLoweringError, MirLoweringOptions, source::lower};
use kagari_abi::types::ConcreteFunctionIdentity;
use kagari_common::{DiagnosticKind, identity::ModuleIdentity};
use kagari_hir::program::CheckedProgram;
use kagari_mir::{
    CallTarget, Instruction,
    program::{ProgramError, ProgramErrorKind, VerifiedMirProgram, verify_program},
};
use std::collections::{HashMap, HashSet};
#[derive(Debug)]
pub enum SourceProgramError {
    Lowering {
        module: Box<ModuleIdentity>,
        error: MirLoweringError,
    },
    Verification(ProgramError),
}
impl From<ProgramError> for SourceProgramError {
    fn from(error: ProgramError) -> Self {
        Self::Verification(error)
    }
}
pub fn lower_program_to_mir(
    program: &CheckedProgram,
    options: &MirLoweringOptions,
) -> Result<VerifiedMirProgram, SourceProgramError> {
    let root = program.root().lowered.source.module_identity().clone();
    let mut requests: HashMap<ModuleIdentity, Vec<ConcreteFunctionIdentity>> = HashMap::new();
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
            let lowered =
                lower::lower_to_mir_with_requests(module, &remaining, demanded, program.modules())
                    .map_err(|mut error| {
                        if let MirLoweringError::Diagnostic(diagnostic) = &mut error
                            && let DiagnosticKind::CompileLimitExceeded { resource, limit } =
                                &mut diagnostic.kind
                        {
                            match *resource {
                                "generated instructions" => *limit = options.max_instructions,
                                "generic instances" => *limit = options.max_generic_instances,
                                _ => {}
                            }
                        }
                        SourceProgramError::Lowering {
                            module: Box::new(identity.clone()),
                            error,
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
                    .count()
                + lowered
                    .interface_instances
                    .iter()
                    .filter(|instance| !instance.arguments.is_empty())
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
            let instance = ConcreteFunctionIdentity {
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
        let allocations = modules.iter().flat_map(|module| {
            module
                .functions
                .iter()
                .flat_map(|function| &function.blocks)
                .flat_map(|block| &block.instructions)
                .filter_map(|instruction| match instruction {
                    Instruction::MakeInterface {
                        implementation,
                        arguments,
                        ..
                    } => Some((
                        &module.identity,
                        ConcreteFunctionIdentity {
                            declaration: implementation.clone(),
                            arguments: arguments.clone(),
                        },
                    )),
                    _ => None,
                })
        });
        let demands = modules.iter().flat_map(|module| {
            module
                .interface_instances
                .iter()
                .cloned()
                .map(|instance| (&module.identity, instance))
        });
        for (caller, instance) in allocations.chain(demands) {
            let implementation = &instance.declaration;
            let arguments = &instance.arguments;
            if arguments.is_empty() || implementation.module == *caller {
                continue;
            }
            options.cancel.check().map_err(|_| ProgramError {
                module: Box::new(root.clone()),
                kind: ProgramErrorKind::Cancelled,
            })?;
            let owner = program
                .modules()
                .iter()
                .find(|module| *module.lowered.source.module_identity() == implementation.module)
                .ok_or_else(|| ProgramError {
                    module: Box::new(root.clone()),
                    kind: ProgramErrorKind::InterfaceContract(implementation.clone()),
                })?;
            let signature = owner
                .aggregates
                .implementation_signature(implementation)
                .ok_or_else(|| ProgramError {
                    module: Box::new(root.clone()),
                    kind: ProgramErrorKind::InterfaceContract(implementation.clone()),
                })?;
            for method in owner.aggregates.implementation_methods(signature) {
                let instance = ConcreteFunctionIdentity {
                    declaration: method.clone(),
                    arguments: arguments.clone(),
                };
                if seen.insert(instance.clone()) {
                    requests
                        .entry(instance.declaration.module.clone())
                        .or_default()
                        .push(instance);
                    changed = true;
                }
            }
        }
        if !changed {
            return verify_program(root, modules, &options.cancel)
                .map_err(SourceProgramError::Verification);
        }
    }
}
