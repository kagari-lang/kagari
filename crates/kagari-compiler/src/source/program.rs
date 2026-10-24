use crate::source::{
    logging, lower,
    lower::{MirLoweringError, instances::MirLoweringOptions},
    program::update_requests::RequestQueue,
    source_package::build_source_manifest,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity, mapping::DefinitionMappingError, metadata::DefinitionMetadata,
        scoped::DefinitionReferences,
    },
};
use kagari_contract::{
    types::{ConcreteFunctionIdentity, PublicItem, named::NamedDeclaration},
    updates::{
        BodyApplication, DemandEnvelope, DemandRequests,
        demands::{
            NativeClosure, check_coverage, expand_requests, publication_demands, union_demands,
        },
        limits::{Budget, UpdateError},
    },
};
use kagari_hir::{
    program::CheckedProgram, resolver::resolved::ResolvedName, typeck::FunctionImplementation,
};
use kagari_mir::{
    function::MirModule,
    instruction::{CallTarget, ConstructionTarget as MirConstruction, Instruction},
    program::{
        ProgramError, ProgramErrorKind, VerifiedMirProgram,
        construction::closed_interface_instances, records::MirProgram, updates, verify_program,
    },
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{
    callable::CallableImplementation,
    ty::{GenericParam, Ty, substitution::TypeTransformError},
};
use std::collections::{BTreeSet, HashMap, HashSet};

mod exports;
mod update_records;
mod update_requests;
mod update_source;

#[derive(Debug)]
pub enum SourceProgramError {
    Lowering {
        module: Box<ModuleIdentity>,
        error: MirLoweringError,
    },
    Verification(ProgramError),
    Update {
        stage: &'static str,
        error: UpdateError,
    },
}

impl From<ProgramError> for SourceProgramError {
    fn from(error: ProgramError) -> Self {
        Self::Verification(error)
    }
}

/// Retain exact checked private trait contracts used by foreign source impls.
/// Source checking already established lexical access. Portable linking consumes
/// the copied requirement rather than resolving source privacy or trait syntax.
fn attach_private_trait_requirements(
    modules: &mut [MirModule],
    cancel: &CancellationToken,
) -> Result<(), MirLoweringError> {
    let mut private = HashMap::new();
    for module in modules.iter() {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        for contract in &module.abi.trait_contracts {
            private.insert(
                contract.declaration,
                (module.identity.clone(), contract.clone()),
            );
        }
    }
    for module in modules {
        let mut retained = module
            .abi
            .trait_references
            .iter()
            .map(|reference| reference.declaration)
            .collect::<HashSet<_>>();
        for item in &module.abi.public_items {
            cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
            let PublicItem::InterfaceTable(table) = item else {
                continue;
            };
            let Ty::Trait(interface) = &table.trait_type else {
                continue;
            };
            if let Some((owner, contract)) = private.get(&interface.declaration)
                && owner != &module.identity
                && retained.insert(contract.declaration)
            {
                module.abi.trait_references.push(contract.clone());
            }
        }
    }
    Ok(())
}

pub fn lower_program_to_mir(
    program: &CheckedProgram,
    options: &MirLoweringOptions,
) -> Result<VerifiedMirProgram, SourceProgramError> {
    lower_program(program, options, None)
}

/// Compile candidate code for all retained installation demands as well as new source.
/// The retained metadata and implementation headers must share their identity scope.
/// Semantic implementation correspondence is resolved before any requests are planned;
/// missing current declarations fail rather than falling back to historical code.
pub fn lower_program_to_mir_with_demands(
    program: &CheckedProgram,
    options: &MirLoweringOptions,
    retained: &DefinitionMetadata<DemandEnvelope>,
    implementation_headers: &[NamedDeclaration],
) -> Result<VerifiedMirProgram, SourceProgramError> {
    lower_program(program, options, Some((retained, implementation_headers)))
}

fn lower_program(
    program: &CheckedProgram,
    options: &MirLoweringOptions,
    retained: Option<(&DefinitionMetadata<DemandEnvelope>, &[NamedDeclaration])>,
) -> Result<VerifiedMirProgram, SourceProgramError> {
    let context = program.root().definition_context();
    let root = program.root().lowered.source.module_identity().clone();
    let source_inputs = update_records::collect(program, &options.cancel).map_err(|error| {
        SourceProgramError::Lowering {
            module: Box::new(root.clone()),
            error,
        }
    })?;
    let source_manifest = build_source_manifest(program, &options.cancel).map_err(|error| {
        SourceProgramError::Lowering {
            module: Box::new(root.clone()),
            error: error.into(),
        }
    })?;
    let mut retained = retained
        .map(|(metadata, headers)| {
            update_requests::import_retained(
                metadata,
                headers,
                &source_inputs.declarations,
                &context,
                &options.cancel,
            )
        })
        .transpose()
        .map_err(|error| SourceProgramError::Lowering {
            module: Box::new(root.clone()),
            error: error.into(),
        })?
        .unwrap_or_default();
    // The root catalog already owns checked facts for the entire dependency
    // closure, including caller-private generic arguments. Per-module catalogs
    // cannot select those arguments' implementations in foreign bodies.
    let authoring_modules = program
        .modules()
        .iter()
        .map(|module| {
            module
                .to_unverified(&options.cancel)
                .map(|metadata| metadata.into_records())
                .map_err(|error| SourceProgramError::Lowering {
                    module: Box::new(root.clone()),
                    error: error.into(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let catalog = &authoring_modules
        .iter()
        .find(|module| *module.lowered.source.module_identity() == root)
        .expect("checked root module")
        .aggregates;
    let exports =
        exports::lower(program, &options.cancel).map_err(|error| SourceProgramError::Lowering {
            module: Box::new(root.clone()),
            error: error.into(),
        })?;
    let layout_requests = exports::layout_requests(&exports, &context.snapshot(), &options.cancel)
        .map_err(|error| SourceProgramError::Lowering {
            module: Box::new(root.clone()),
            error: error.into(),
        })?;
    let mut requests = RequestQueue::default();
    requests.layouts(layout_requests);
    let mut seen = HashSet::new();
    let mut update_budget = Budget::new(Default::default(), &options.cancel).map_err(|error| {
        SourceProgramError::Update {
            stage: "initialize update budget",
            error,
        }
    })?;
    loop {
        options.cancel.check().map_err(|_| ProgramError {
            module: Box::new(root.clone()),
            kind: ProgramErrorKind::Cancelled,
        })?;
        let mut modules = Vec::new();
        let mut classifications = Vec::new();
        let mut source_bodies = Vec::new();
        let mut remaining = options.clone();
        for module in &authoring_modules {
            let identity = module.lowered.source.module_identity();
            let handle = module
                .names
                .catalog
                .definitions()
                .lookup_module(identity)
                .expect("checked source module");
            let empty_requests = DemandRequests::default();
            let demanded = requests.module(handle).unwrap_or(&empty_requests);
            let lower::LoweredModule {
                module: lowered,
                classifications: module_classifications,
                source_bodies: module_source_bodies,
            } = lower::lower_to_mir_with_requests(
                module,
                &remaining,
                demanded,
                &authoring_modules,
                catalog,
                &context,
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
            classifications.extend(module_classifications);
            source_bodies.extend(module_source_bodies);

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
            modules.push(lowered.into_unverified().into_records());
        }
        attach_private_trait_requirements(&mut modules, &options.cancel).map_err(|error| {
            SourceProgramError::Lowering {
                module: Box::new(root.clone()),
                error,
            }
        })?;
        logging::attach(&mut modules, &context, &options.cancel).map_err(|error| {
            SourceProgramError::Lowering {
                module: Box::new(root.clone()),
                error,
            }
        })?;
        let definitions = context.snapshot();
        let owner_of = |id| {
            definitions
                .resolve(id)
                .expect("lowered definition identity")
                .module()
        };
        let mut changed = false;
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
                                owner_of(target.instance.declaration) == &module.identity
                            })
                            .map(|target| &target.instance),
                    )
            })
            .collect();
        let callable_requests = modules.iter().map(callable_demands).collect::<Vec<_>>();
        for application in callable_requests
            .iter()
            .flat_map(|requests| &requests.shared)
        {
            options.cancel.check().map_err(|_| ProgramError {
                module: Box::new(root.clone()),
                kind: ProgramErrorKind::Cancelled,
            })?;
            if !materialized.contains(&application.body) {
                changed |= requests
                    .shared(application.clone(), &definitions)
                    .map_err(|error| SourceProgramError::Lowering {
                        module: Box::new(root.clone()),
                        error,
                    })?;
            }
        }
        for instance in callable_requests
            .into_iter()
            .flat_map(|requests| requests.functions)
        {
            options.cancel.check().map_err(|_| ProgramError {
                module: Box::new(root.clone()),
                kind: ProgramErrorKind::Cancelled,
            })?;
            if !materialized.contains(&instance) && seen.insert(instance.clone()) {
                requests.function(instance, &definitions).map_err(|error| {
                    SourceProgramError::Lowering {
                        module: Box::new(root.clone()),
                        error,
                    }
                })?;
                changed = true;
            }
        }
        let constructions = closed_interface_instances(
            &modules.iter().collect::<Vec<_>>(),
            &definitions,
            options.max_generic_instances.saturating_add(
                modules
                    .iter()
                    .map(|module| {
                        module.functions.len()
                            + module
                                .native_applications()
                                .filter(|import| import.result_adapter.is_some())
                                .count()
                    })
                    .sum::<usize>(),
            ),
            &options.cancel,
        )
        .map_err(|error| SourceProgramError::Lowering {
            module: Box::new(root.clone()),
            error: match error {
                TypeTransformError::Cancelled => MirLoweringError::Cancelled,
                TypeTransformError::LimitExceeded => MirLoweringError::Diagnostic(Box::new(
                    Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                        resource: "generic instances",
                        limit: options.max_generic_instances,
                    })
                    .with_span(Default::default()),
                )),
                TypeTransformError::InvalidContract => {
                    MirLoweringError::MissingBinding("shared construction contract")
                }
            },
        })?;
        for instance in constructions {
            let owner = owner_of(instance.declaration);
            if let Some(module) = modules.iter_mut().find(|module| &module.identity == owner)
                && module.abi.public_items.iter().any(|item| {
                    matches!(item,
                    PublicItem::InterfaceTable(table) if table.declaration == instance.declaration
                        && table.bridge.is_some() && !table.generic_params.is_empty())
                })
            {
                // Generated intrinsic tables share their executable method bodies.
                // Closing an application only materializes a checked table record.
                if !module.interface_instances.contains(&instance) {
                    if remaining.max_generic_instances == 0 {
                        return Err(SourceProgramError::Lowering {
                            module: Box::new(root.clone()),
                            error: MirLoweringError::Diagnostic(Box::new(
                                Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                                    resource: "generic instances",
                                    limit: options.max_generic_instances,
                                })
                                .with_span(Default::default()),
                            )),
                        });
                    }
                    remaining.max_generic_instances -= 1;
                    module.interface_instances.push(instance);
                }
                continue;
            }
            let materialized = modules
                .iter()
                .find(|module| &module.identity == owner)
                .is_some_and(|module| module.interface_instances.contains(&instance));
            if !materialized && seen.insert(instance.clone()) {
                requests
                    .construction(instance, &definitions)
                    .map_err(|error| SourceProgramError::Lowering {
                        module: Box::new(root.clone()),
                        error,
                    })?;
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
                        target:
                            MirConstruction::Declared {
                                implementation,
                                arguments,
                                ..
                            },
                        ..
                    } => Some((
                        &module.identity,
                        ConcreteFunctionIdentity {
                            declaration: *implementation,
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
                .map(|instance| (&module.identity, instance.clone()))
        });
        for (caller, instance) in allocations.chain(demands) {
            let implementation = &instance.declaration;
            let arguments = &instance.arguments;
            if arguments.is_empty() || owner_of(*implementation) == caller {
                continue;
            }
            options.cancel.check().map_err(|_| ProgramError {
                module: Box::new(root.clone()),
                kind: ProgramErrorKind::Cancelled,
            })?;
            let owner = authoring_modules
                .iter()
                .find(|module| module.lowered.source.module_identity() == owner_of(*implementation))
                .ok_or_else(|| ProgramError {
                    module: Box::new(root.clone()),
                    kind: ProgramErrorKind::InterfaceContract(
                        definitions
                            .resolve(*implementation)
                            .expect("checked implementation")
                            .to_path(),
                    ),
                })?;
            let signature = owner
                .aggregates
                .implementation_signature(implementation)
                .ok_or_else(|| ProgramError {
                    module: Box::new(root.clone()),
                    kind: ProgramErrorKind::InterfaceContract(
                        definitions
                            .resolve(*implementation)
                            .expect("checked implementation")
                            .to_path(),
                    ),
                })?;
            for method in owner
                .aggregates
                .implementation_methods(&definitions, signature)
            {
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
                            definitions
                                .resolve(required.id)
                                .expect("checked method")
                                .last()
                                == definitions.resolve(method).expect("checked method").last()
                                && required.generic_params.len() > contract.generic_params.len()
                                && required
                                    .params
                                    .first()
                                    .is_none_or(|parameter| parameter.name != "self")
                        })
                    })
                {
                    // Static generic methods do not occupy an interface receiver slot.
                    continue;
                }
                // A table can expose methods whose additional bounds are proved
                // only at a call site. Keep their receiver binders in a shared
                // native entry rather than demanding an uncallable specialization.
                let conditional = owner
                    .declarations
                    .definition_target(&method)
                    .and_then(|target| match target {
                        ResolvedName::Function(id) => {
                            owner.typed.functions.iter().find(|f| f.id == id)
                        }
                        _ => None,
                    })
                    .is_some_and(|function| {
                        matches!(function.implementation, FunctionImplementation::Native(_))
                            && function.bounds.iter().any(|(ty, constraints)| {
                                constraints.iter().any(|constraint| {
                                    !signature
                                        .bounds
                                        .get(ty)
                                        .is_some_and(|assumed| assumed.contains(constraint))
                                })
                            })
                    });
                let instance = ConcreteFunctionIdentity {
                    declaration: method,
                    arguments: if conditional {
                        arguments
                            .iter()
                            .enumerate()
                            .map(|(position, _)| {
                                GenericParam {
                                    owner: method,
                                    position,
                                }
                                .as_type()
                            })
                            .collect()
                    } else {
                        arguments.clone()
                    },
                };
                if seen.insert(instance.clone()) {
                    requests.function(instance, &definitions).map_err(|error| {
                        SourceProgramError::Lowering {
                            module: Box::new(root.clone()),
                            error,
                        }
                    })?;
                    changed = true;
                }
            }
        }
        let source_owners: HashSet<_> = modules
            .iter()
            .map(|module| module.identity.clone())
            .collect();
        for module in &mut modules {
            // Concrete generic arguments and selected witnesses can introduce owners
            // absent from the defining source's imports. Retain every referenced
            // source declaration; host identities belong to the host contract table.
            let mut dependencies: BTreeSet<_> = module.dependencies.iter().cloned().collect();
            module
                .visit_definition_ids(
                    &mut |definition| {
                        if owner_of(*definition) != &module.identity
                            && source_owners.contains(owner_of(*definition))
                        {
                            dependencies.insert(owner_of(*definition).clone());
                        }
                        Ok(())
                    },
                    &options.cancel,
                )
                .map_err(|error| ProgramError {
                    module: Box::new(module.identity.clone()),
                    kind: if matches!(error, DefinitionMappingError::Cancelled) {
                        ProgramErrorKind::Cancelled
                    } else {
                        ProgramErrorKind::InvalidGraph
                    },
                })?;
            module.dependencies = dependencies.into_iter().collect();
        }
        if !changed {
            let mut evidence = updates::build(
                &modules,
                updates::EvidenceInputs {
                    sources: update_records::attach_bodies(
                        &source_inputs.sources,
                        source_bodies,
                        &options.cancel,
                    )
                    .map_err(|error| SourceProgramError::Lowering {
                        module: Box::new(root.clone()),
                        error: error.into(),
                    })?,
                    initializers: source_inputs.initializers.clone(),
                    selections: vec![],
                    classifications: classifications.into_iter().collect(),
                },
                &definitions,
                &options.cancel,
            )
            .map_err(|error| SourceProgramError::Update {
                stage: "capture preoptimization evidence",
                error,
            })?;
            evidence.source_manifest = Some(source_manifest.clone());
            let (modules, evidence) = update_records::optimize(
                modules, evidence, &context, options,
            )
            .map_err(|error| SourceProgramError::Lowering {
                module: Box::new(root.clone()),
                error,
            })?;
            let candidate = verify_program(
                MirProgram {
                    root: root.clone(),
                    modules,
                    exports: exports.clone(),
                    update_evidence: evidence,
                },
                &context,
                &options.cancel,
            )
            .map_err(SourceProgramError::Verification)?;
            let current = publication_demands(candidate.update_evidence(), &mut update_budget)
                .map_err(|error| SourceProgramError::Update {
                    stage: "collect publication demands",
                    error,
                })?;
            retained = union_demands(
                &retained,
                &current,
                candidate.definitions(),
                &mut update_budget,
            )
            .map_err(|error| SourceProgramError::Update {
                stage: "union retained demands",
                error,
            })?;
            let natives = candidate
                .modules()
                .iter()
                .flat_map(|module| module.native_applications())
                .map(|import| NativeClosure {
                    import,
                    callbacks: None,
                })
                .collect::<Vec<_>>();
            let mut coverage = candidate.update_coverage();
            coverage.natives = &natives;
            let proofs = update_requests::proof_catalog(
                candidate.modules(),
                candidate.definitions(),
                &options.cancel,
            )
            .map_err(|error| SourceProgramError::Update {
                stage: "prepare candidate retained-demand proofs",
                error: error.into(),
            })?;
            let batch = expand_requests(&retained, &coverage, &proofs, &mut update_budget)
                .map_err(|error| SourceProgramError::Update {
                    stage: "expand retained requests",
                    error,
                })?;
            if batch.is_empty() {
                check_coverage(&retained, &coverage, &proofs, &mut update_budget).map_err(
                    |error| SourceProgramError::Update {
                        stage: "validate final retained coverage",
                        error,
                    },
                )?;
                return Ok(candidate);
            }
            let owner = context
                .snapshot()
                .lookup_module(&root)
                .expect("checked root module");
            if !requests
                .apply(
                    batch,
                    catalog,
                    candidate.definitions(),
                    owner,
                    &options.cancel,
                )
                .map_err(|error| SourceProgramError::Lowering {
                    module: Box::new(root.clone()),
                    error,
                })?
            {
                // An unchanged batch cannot gain coverage by repeating lowering.
                return Err(SourceProgramError::Update {
                    stage: "materialize retained requests",
                    error: UpdateError::MissingCoverage,
                });
            }
        }
    }
}

// Already emitted shared bodies need no source declaration demand. Generated
// adapters have executable contracts but deliberately have no authored declaration.
fn callable_demands(module: &MirModule) -> DemandRequests {
    let mut requests = DemandRequests::default();
    for instruction in module
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
    {
        match instruction {
            Instruction::Call {
                callee: CallTarget::Shared(contract),
                ..
            } if contract.implementation == CallableImplementation::Script => {
                if !module
                    .functions
                    .iter()
                    .any(|function| function.instance == contract.instance)
                {
                    requests.shared.push(BodyApplication {
                        body: contract.instance.clone(),
                        arguments: contract.arguments.clone(),
                    });
                }
            }
            Instruction::Call {
                callee: CallTarget::Shared(contract),
                ..
            } => {
                requests.functions.push(contract.instance.clone());
            }
            Instruction::Call {
                callee: CallTarget::SourceFunction(contract),
                ..
            } => {
                requests.functions.push(ConcreteFunctionIdentity {
                    declaration: contract.declaration,
                    arguments: contract.arguments.clone(),
                });
            }
            _ => {}
        }
    }
    requests.functions.extend(
        module
            .selected_callables()
            .map(|call| call.instance.clone()),
    );
    requests
}
