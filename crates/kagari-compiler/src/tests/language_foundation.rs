use crate::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::catalog as foundation_catalog;
use rstest::rstest;

#[rstest]
#[case::scalar_language_program_compiles_without_optional_modules(include_str!(
    "../../tests/fixtures/language_foundation/scalar-language.kgr"
))]
#[case::default_list_native_calls_compile_without_optional_modules(include_str!(
    "../../tests/fixtures/language_foundation/default-list-native.kgr"
))]
#[case::collection_interface_iteration_compiles_without_optional_modules(include_str!(
    "../../tests/fixtures/language_foundation/collection-interface-iteration.kgr"
))]
#[case::foundational_static_methods_and_generic_bounds_compile(include_str!(
    "../../tests/fixtures/language_foundation/foundational-static-methods-and-bounds.kgr"
))]
fn compile_success(#[case] text: &str) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("foundation.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(foundation_catalog::shared().expect("foundation declarations"));
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    kagari_bytecode::program::verify_program(
        bytecode.records(),
        bytecode.definitions(),
        &Default::default(),
    )
    .unwrap();
}
