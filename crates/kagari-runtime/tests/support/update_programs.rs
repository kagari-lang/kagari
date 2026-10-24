//! Shared source/artifact preparation for runtime unit and integration contracts.
use super::{
    ExecutableModule, ReloadValidationError, VerifiedProgram, validate_reload_artifact_candidate,
    validate_reload_candidate,
};
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::program::{lower_program_to_mir, lower_program_to_mir_with_demands},
};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

pub(super) fn source_code(text: &str) -> VerifiedProgram {
    VerifiedProgram::new(bytecode(text, None), &DefinitionContext::new().unwrap()).unwrap()
}

pub(super) fn bytecode(
    text: &str,
    baseline: Option<&ExecutableModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    let mut source = SourceDatabase::default();
    let root = source
        .set("exports.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let checked = analysis
        .snapshot(source.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = match baseline {
        None => lower_program_to_mir(&checked, &Default::default()),
        Some(baseline) => {
            let retained = baseline.retained_demands();
            lower_program_to_mir_with_demands(
                &checked,
                &Default::default(),
                retained.metadata(),
                retained.implementation_headers(),
            )
        }
    }
    .unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified()
}

pub(super) fn source_free_reload_results(
    loaded: &ExecutableModule,
    source: &str,
) -> [Result<(), ReloadValidationError>; 2] {
    source_free_program_results(loaded, bytecode(source, None))
}

pub(super) fn source_free_artifact(
    input: DefinitionMetadata<BytecodeProgram>,
) -> DefinitionMetadata<KbcArtifact> {
    let context = DefinitionContext::new().unwrap();
    let program = input
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let artifact =
        KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    KbcArtifact::from_bytes(&bytes).unwrap()
}

pub(super) fn source_free_program_results(
    loaded: &ExecutableModule,
    input: DefinitionMetadata<BytecodeProgram>,
) -> [Result<(), ReloadValidationError>; 2] {
    let artifact = source_free_artifact(input.clone());
    let context = DefinitionContext::new().unwrap();
    let program = input
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let raw = validate_reload_candidate(loaded, program, Some(loaded), &context);
    let decoded_context = DefinitionContext::new().unwrap();
    let decoded = artifact
        .import_into(&decoded_context, &Default::default())
        .unwrap()
        .into_records();
    let artifact = validate_reload_artifact_candidate(
        loaded,
        decoded,
        &Default::default(),
        Some(loaded),
        &decoded_context,
    );
    [raw.map(|_| ()), artifact.map(|_| ())]
}
