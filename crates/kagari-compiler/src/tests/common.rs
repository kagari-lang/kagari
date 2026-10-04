use crate::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_bytecode::program::BytecodeProgram;
use kagari_contract::library::catalog as foundation_catalog;
use kagari_hir::{
    CheckedAnalysis, analysis::AnalysisDatabase, analyze_source, program::CheckedProgram,
};
use kagari_mir::{
    program::{VerifiedMirProgram, verify_program},
    verify::VerifiedMirModule,
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};

pub fn analyze_ok(text: &str) -> Box<CheckedAnalysis> {
    let source = SourceFile::new("test.kg", text);

    Box::new(
        analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis")
            .into_codegen()
            .expect("analysis should succeed"),
    )
}

pub fn program_ok(text: &str) -> CheckedProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("test.kg", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analysis_database()
        .snapshot(sources.snapshot(), &Default::default())
        .expect("analysis snapshot should succeed");
    snapshot
        .check_program(root, &Default::default())
        .expect("program analysis should succeed")
}

pub fn mir_ok(text: &str) -> VerifiedMirProgram {
    let checked = program_ok(text);
    lower_program_to_mir(&checked, &Default::default()).expect("program IR lowering should succeed")
}

pub fn bytecode_ok(text: &str) -> BytecodeProgram {
    lower_program_to_bytecode(&mir_ok(text)).expect("program bytecode lowering should succeed")
}

/// Module transformation tests revalidate the edited root within its original
/// checked dependency closure, including cross-module contracts and native proofs.
pub fn bytecode_with_edited_root(
    checked: &CheckedProgram,
    edited: &VerifiedMirModule,
) -> BytecodeProgram {
    let original = lower_program_to_mir(checked, &Default::default())
        .expect("program MIR lowering should succeed");
    assert_eq!(original.root(), &edited.identity);
    let root = original.root().clone();
    let mut members = original.into_unverified();
    let slot = members
        .iter()
        .position(|member| member.identity == root)
        .unwrap();
    members[slot] = edited.clone().into_unverified();
    let verified =
        verify_program(root, members, &Default::default()).expect("edited program should verify");
    lower_program_to_bytecode(&verified).expect("edited program bytecode lowering should succeed")
}

fn analysis_database() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(foundation_catalog::shared());
    database
}
