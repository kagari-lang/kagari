use crate::source::lower::{self, MirLoweringError, instances::MirLoweringOptions};
use kagari_abi::types::ConcreteFunctionIdentity;
use kagari_common::{diagnostic::DiagnosticKind, identity::ModuleIdentity};
use kagari_hir::program::CheckedProgram;
use kagari_hir::{resolver::resolved::ResolvedName, typeck::FunctionImplementation};
use kagari_mir::{
    function::MirModule,
    instruction::{CallTarget, Instruction},
    program::{ProgramError, ProgramErrorKind, VerifiedMirProgram, verify_program},
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    iter,
};
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
    // The root catalog already owns checked facts for the entire dependency
    // closure, including caller-private generic arguments. Per-module catalogs
    // cannot select those arguments' implementations in foreign bodies.
    let catalog = &program.root().aggregates;
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
            let lowered = lower::lower_to_mir_with_requests(
                module,
                &remaining,
                demanded,
                program.modules(),
                catalog,
            )
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
                    .count()
                + lowered
                    .native_targets
                    .iter()
                    .filter(|target| !target.instance.arguments.is_empty())
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
        for module in &mut modules {
            // A generic body may use a private implementation supplied by its
            // caller. Pin both invoked methods and non-invoked bound witnesses
            // even when the defining source module did not import the caller.
            let mut dependencies: BTreeSet<_> = module.dependencies.iter().cloned().collect();
            for dependency in execution_dependencies(module) {
                options.cancel.check().map_err(|_| ProgramError {
                    module: Box::new(root.clone()),
                    kind: ProgramErrorKind::Cancelled,
                })?;
                if dependency != module.identity {
                    dependencies.insert(dependency);
                }
            }
            module.dependencies = dependencies.into_iter().collect();
        }
        let materialized: HashSet<_> = modules
            .iter()
            .flat_map(|module| {
                module
                    .functions
                    .iter()
                    .map(|function| &function.instance)
                    .chain(
                        module
                            .native_targets
                            .iter()
                            .filter(|target| {
                                // A foreign target retained for a local interface slot
                                // does not publish the callback target in its owner.
                                target.instance.declaration.module == module.identity
                            })
                            .map(|target| &target.instance),
                    )
            })
            .collect();
        for instance in modules.iter().flat_map(callable_demands) {
            options.cancel.check().map_err(|_| ProgramError {
                module: Box::new(root.clone()),
                kind: ProgramErrorKind::Cancelled,
            })?;
            if !materialized.contains(&instance) && seen.insert(instance.clone()) {
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
                if let Some(ResolvedName::Function(function)) =
                    owner.declarations.definition_target(&method)
                    && owner.typed.functions.iter().any(|typed| {
                        typed.id == function
                            && matches!(typed.implementation, FunctionImplementation::Native(_))
                    })
                    && !owner
                        .lowered
                        .registered_native_declarations()
                        .iter()
                        .any(|declaration| declaration.declaration == method)
                {
                    // Legacy engine/host adapters retain their existing lowering.
                    continue;
                }
                if owner
                    .aggregates
                    .trait_(&signature.trait_type.declaration)
                    .is_some_and(|contract| {
                        contract.methods.iter().any(|required| {
                            required.id.path.last() == method.path.last()
                                && required.generic_params.len() > contract.generic_params.len()
                        })
                    })
                {
                    // Method generics need the concrete application carried by
                    // a source call or native witness, not just impl arguments.
                    continue;
                }
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

fn execution_dependencies(module: &MirModule) -> impl Iterator<Item = ModuleIdentity> + '_ {
    let callables = callable_demands(module).map(|instance| instance.declaration.module);
    let native = module
        .native_applications()
        .filter(|contract| contract.host.is_none())
        .flat_map(|contract| {
            iter::once(contract.instance.declaration.module.clone()).chain(
                contract
                    .callables
                    .iter()
                    .map(|callable| callable.instance.declaration.module.clone()),
            )
        });
    callables.chain(native).chain(
        module
            .interface_instances
            .iter()
            .map(|instance| instance.declaration.module.clone()),
    )
}

fn callable_demands(module: &MirModule) -> impl Iterator<Item = ConcreteFunctionIdentity> + '_ {
    let script = module
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .flat_map(|instruction| match instruction {
            Instruction::Call {
                callee: CallTarget::SourceFunction(contract),
                ..
            } => vec![ConcreteFunctionIdentity {
                declaration: contract.declaration.clone(),
                arguments: contract.arguments.clone(),
            }],
            _ => vec![],
        });
    script.chain(
        module
            .native_applications()
            .flat_map(|import| &import.callables)
            .map(|call| call.instance.clone()),
    )
}
