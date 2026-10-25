//! Checked source inputs, retained scheduling and candidate coverage orchestration.

use crate::source::{
    lower::{MirLoweringError, instances::MirLoweringOptions},
    program::{candidate::CandidateInputs, scheduler::Scheduler, update_requests::RequestQueue},
    source_package::build_source_manifest,
};
use kagari_common::identity::{ModuleIdentity, metadata::DefinitionMetadata};
use kagari_contract::{
    types::named::NamedDeclaration,
    updates::{
        DemandEnvelope,
        limits::{Budget, UpdateError},
    },
};
use kagari_hir::program::CheckedProgram;
use kagari_mir::program::{ProgramError, VerifiedMirProgram};
#[cfg(test)]
use std::time::Instant;

mod candidate;
mod discovery;
mod exports;
mod interface_requests;
mod scheduler;
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
    let mut scheduler = Scheduler::new(
        &authoring_modules,
        catalog,
        &context,
        &root,
        options,
        requests,
    );
    let inputs = CandidateInputs {
        root: &root,
        context: &context,
        options,
        sources: &source_inputs,
        source_manifest: &source_manifest,
        exports: &exports,
    };
    let mut update_budget = Budget::new(Default::default(), &options.cancel).map_err(|error| {
        SourceProgramError::Update {
            stage: "initialize update budget",
            error,
        }
    })?;
    loop {
        scheduler.close()?;
        #[cfg(test)]
        let candidate_start = Instant::now();
        let candidate = candidate::build(scheduler.snapshot()?, &inputs)?;
        #[cfg(test)]
        crate::source::work_metrics::candidate_work(candidate_start.elapsed());
        let batch = candidate::expand(&candidate, &mut retained, &mut update_budget, &inputs)?;
        if batch.is_empty() {
            return Ok(candidate);
        }
        if !scheduler.apply(batch, candidate.definitions())? {
            // Reassembling identical products cannot gain missing retained coverage.
            return Err(SourceProgramError::Update {
                stage: "materialize retained requests",
                error: UpdateError::MissingCoverage,
            });
        }
    }
}
