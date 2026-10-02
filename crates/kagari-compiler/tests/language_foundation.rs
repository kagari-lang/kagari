use kagari_common::source_database::{SourceDatabase, SourceLayer};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;

fn compile(text: &str) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("foundation.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir).unwrap();
    kagari_bytecode::program::verify_program(&bytecode).unwrap();
}

#[test]
fn scalar_language_program_compiles_without_optional_modules() {
    compile("fn main() -> i32 { if 1 + 2 == 3 { 42 } else { 0 } }");
}

#[test]
fn default_list_native_calls_compile_without_optional_modules() {
    compile("fn main() -> i32 { val values = [1, 2]; values.push(3); values[0] }");
}

#[test]
fn collection_interface_iteration_compiles_without_optional_modules() {
    compile(
        "fn main() -> i32 { val values: List<i32> = [1, 2]; var total = 0; for item in values { total = total + item; } total }",
    );
}
