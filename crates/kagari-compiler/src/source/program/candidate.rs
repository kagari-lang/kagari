//! Candidate-only metadata completion, optimization and independent proof coverage.

use crate::source::{
    logging,
    lower::{MirLoweringError, instances::MirLoweringOptions},
    program::{
        SourceProgramError,
        scheduler::ProgramSnapshot,
        update_records::{self, SourceInputs},
        update_requests,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity, map::DefinitionContext, mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
    },
};
use kagari_contract::{
    exports::ProgramExports,
    source::manifest::SourceManifest,
    types::PublicItem,
    updates::{
        DemandEnvelope, DemandRequests,
        demands::{
            NativeClosure, check_coverage, expand_requests, publication_demands, union_demands,
        },
        limits::Budget,
    },
};
use kagari_mir::{
    function::MirModule,
    program::{
        ProgramError, ProgramErrorKind, VerifiedMirProgram, records::MirProgram, updates,
        verify_program,
    },
};
use kagari_types::ty::Ty;
use std::collections::{BTreeSet, HashMap, HashSet};

pub(super) struct CandidateInputs<'a> {
    pub root: &'a ModuleIdentity,
    pub context: &'a DefinitionContext,
    pub options: &'a MirLoweringOptions,
    pub sources: &'a SourceInputs,
    pub source_manifest: &'a SourceManifest,
    pub exports: &'a ProgramExports,
}

pub(super) fn build(
    snapshot: ProgramSnapshot,
    inputs: &CandidateInputs<'_>,
) -> Result<VerifiedMirProgram, SourceProgramError> {
    let ProgramSnapshot {
        mut modules,
        classifications,
        source_bodies,
    } = snapshot;
    let root = inputs.root;
    let context = inputs.context;
    let options = inputs.options;
    let source_inputs = inputs.sources;
    attach_private_trait_requirements(&mut modules, &options.cancel).map_err(|error| {
        SourceProgramError::Lowering {
            module: Box::new(root.clone()),
            error,
        }
    })?;
    logging::attach(&mut modules, context, &options.cancel).map_err(|error| {
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
    evidence.source_manifest = Some(inputs.source_manifest.clone());
    let (modules, evidence) = update_records::optimize(modules, evidence, context, options)
        .map_err(|error| SourceProgramError::Lowering {
            module: Box::new(root.clone()),
            error,
        })?;
    #[cfg(test)]
    crate::source::work_metrics::candidate();
    let candidate = verify_program(
        MirProgram {
            root: root.clone(),
            modules,
            exports: inputs.exports.clone(),
            update_evidence: evidence,
        },
        context,
        &options.cancel,
    )
    .map_err(SourceProgramError::Verification)?;
    Ok(candidate)
}

pub(super) fn expand(
    candidate: &VerifiedMirProgram,
    retained: &mut DemandEnvelope,
    update_budget: &mut Budget<'_>,
    inputs: &CandidateInputs<'_>,
) -> Result<DemandRequests, SourceProgramError> {
    let options = inputs.options;
    let current =
        publication_demands(candidate.update_evidence(), update_budget).map_err(|error| {
            SourceProgramError::Update {
                stage: "collect publication demands",
                error,
            }
        })?;
    *retained = union_demands(retained, &current, candidate.definitions(), update_budget).map_err(
        |error| SourceProgramError::Update {
            stage: "union retained demands",
            error,
        },
    )?;
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
    let batch = expand_requests(retained, &coverage, &proofs, update_budget).map_err(|error| {
        SourceProgramError::Update {
            stage: "expand retained requests",
            error,
        }
    })?;
    if batch.is_empty() {
        check_coverage(retained, &coverage, &proofs, update_budget).map_err(|error| {
            SourceProgramError::Update {
                stage: "validate final retained coverage",
                error,
            }
        })?;
        return Ok(batch);
    }
    Ok(batch)
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
