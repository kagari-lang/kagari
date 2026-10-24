use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::native::module::NativeModule;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::sync::Arc;

pub(super) mod native;

/// Compile an unencoded program from the caller's exact native modules and source identity.
pub(super) fn compile_program(
    modules: &[NativeModule],
    source_name: &str,
    source: &str,
) -> DefinitionMetadata<BytecodeProgram> {
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(source_name, source.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
}
