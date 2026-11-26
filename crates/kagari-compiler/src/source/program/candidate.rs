//! Candidate-only metadata completion, optimization and independent proof coverage.

use crate::source::{
    lower::instances::MirLoweringOptions,
    program::{
        InstallationTraitRequirements, SourceProgramError,
        scheduler::ProgramSnapshot,
        trait_requirements,
        update_records::{self, SourceInputs},
        update_requests,
    },
};
use kagari_common::identity::{ModuleIdentity, map::DefinitionContext};
use kagari_contract::{
    exports::ProgramExports,
    source::manifest::SourceManifest,
    updates::{
        DemandEnvelope, DemandRequests,
        demands::{
            NativeClosure, check_coverage, expand_requests, publication_demands, union_demands,
        },
        limits::Budget,
    },
};
use kagari_mir::program::{VerifiedMirProgram, records::MirProgram, updates, verify_program};

pub(super) struct CandidateInputs<'a> {
    pub installation: &'a InstallationTraitRequirements,
    pub root: &'a ModuleIdentity,
    pub context: &'a DefinitionContext,
    pub options: &'a MirLoweringOptions,
    pub sources: &'a SourceInputs,
    pub source_manifest: Option<&'a SourceManifest>,
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
    let definitions = context.snapshot();
    trait_requirements::attach(
        &mut modules,
        inputs.installation,
        &definitions,
        &options.cancel,
    )?;

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
    evidence.source_manifest = inputs.source_manifest.cloned();
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
