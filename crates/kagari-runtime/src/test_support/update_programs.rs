//! Source/artifact preparation for runtime update and current-call contracts.
use crate::{
    Runtime,
    module::{ExecutableModule, InstalledProgram, VerifiedProgram},
    native::assembly::{ModuleDefinition, ModuleSetBuilder},
    reload::{
        ReloadValidationError, validate_reload_artifact_candidate, validate_reload_candidate,
    },
    test_support::{bytecode::load_bytecode, native::program::ProgramFixture},
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::program::{lower_program_to_mir, lower_program_to_mir_with_demands},
};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::declaration::native::NativeTypeConstructor;
use std::sync::Arc;

pub(crate) fn source_code(text: &str) -> VerifiedProgram {
    VerifiedProgram::new(bytecode(text, None), &DefinitionContext::new().unwrap()).unwrap()
}

pub(crate) fn bytecode(
    text: &str,
    baseline: Option<&ExecutableModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    let mut source = SourceDatabase::default();
    let root = source
        .set("exports.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    // Captures use the core String representation through an application-owned
    // constructor declaration, not an implicitly installed standard library.
    let mut providers = ModuleSetBuilder::new(&Default::default()).unwrap();
    providers
        .add(ModuleDefinition::new(
            ModuleIdentity::single_file("capture-text"),
            |module| {
                module.prelude(true);
                module.define_constructor("String", NativeTypeConstructor::String)?;
                Ok(())
            },
        ))
        .unwrap();
    let providers = providers.finish().unwrap();
    analysis.set_native_modules(
        providers
            .declarations()
            .modules()
            .iter()
            .map(|module| Arc::new(module.clone()))
            .collect(),
    );
    let checked = analysis
        .snapshot(source.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = match baseline {
        None => lower_program_to_mir(
            &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
            &Default::default(),
        ),
        Some(baseline) => {
            let retained = baseline.retained_demands();
            lower_program_to_mir_with_demands(
                &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
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

pub(crate) fn source_free_reload_results(
    loaded: &ExecutableModule,
    source: &str,
) -> [Result<(), ReloadValidationError>; 2] {
    source_free_program_results(loaded, bytecode(source, None))
}

pub(crate) fn source_free_artifact(
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

pub(crate) fn source_free_program_results(
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

pub(crate) fn load_empty(runtime: &mut Runtime, name: &str) -> InstalledProgram {
    let context = DefinitionContext::new().unwrap();
    load_bytecode(
        runtime,
        name,
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                ..Default::default()
            }],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap()
}
