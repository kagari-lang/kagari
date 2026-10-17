use crate::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_bytecode::program::{BytecodeProgram, identity};
use kagari_common::identity::{
    DefinitionPath, map::DefinitionContext, metadata::DefinitionMetadata, scoped::RemapDefinitions,
    table::DefinitionId,
};
use kagari_hir::{
    CheckedAnalysis, analysis::AnalysisDatabase, analyze_source, program::CheckedProgram,
};
use kagari_mir::{
    function::MirModule,
    program::{VerifiedMirProgram, verify_program},
    verify::{MirVerificationError, VerifiedMirModule, verify_mir},
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog as foundation_catalog;

pub fn analyze_ok(text: &str) -> Box<CheckedAnalysis> {
    let source = SourceFile::new("test.kg", text);

    Box::new(
        analyze_source(&source, foundation_catalog::shared().unwrap())
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

pub fn bytecode_ok(text: &str) -> DefinitionMetadata<BytecodeProgram> {
    lower_program_to_bytecode(&mir_ok(text)).expect("program bytecode lowering should succeed")
}

/// Module transformation tests revalidate the edited root within its original
/// checked dependency closure, including cross-module contracts and native proofs.
pub fn bytecode_with_edited_root(
    checked: &CheckedProgram,
    edited: &VerifiedMirModule,
) -> DefinitionMetadata<BytecodeProgram> {
    let original = lower_program_to_mir(checked, &Default::default())
        .expect("program MIR lowering should succeed");
    assert_eq!(original.root(), &edited.identity);
    let root = original.root().clone();
    let context = DefinitionContext::new().unwrap();
    let mut members = original
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let slot = members
        .modules
        .iter()
        .position(|member| member.identity == root)
        .unwrap();
    members.modules[slot] = edited
        .clone()
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let verified = verify_program(members, &context, &Default::default())
        .expect("edited program should verify");
    lower_program_to_bytecode(&verified).expect("edited program bytecode lowering should succeed")
}

fn analysis_database() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(foundation_catalog::shared().unwrap());
    database
}

/// Adopt editable fixture records into the context retained by the calling test.
/// This preserves reference validation; the test still invokes the real verifier.
pub fn adopt<T: RemapDefinitions>(
    context: &DefinitionContext,
    metadata: DefinitionMetadata<T>,
) -> T {
    metadata
        .import_into(context, &Default::default())
        .unwrap()
        .into_records()
}

/// Build a deliberately altered authoring identity for verifier rejection fixtures.
/// Only the selected reference changes; the semantic record still stores an ID.
pub fn edit_definition(
    context: &DefinitionContext,
    id: &mut DefinitionId,
    edit: impl FnOnce(&mut DefinitionPath),
) {
    let mut path = context.snapshot().resolve(*id).unwrap().to_path();
    edit(&mut path);
    *id = context.intern(&path).unwrap();
}

/// Register the implicit ABI names needed by source-free bytecode verification.
pub fn adopt_bytecode(
    context: &DefinitionContext,
    metadata: DefinitionMetadata<BytecodeProgram>,
) -> BytecodeProgram {
    let program = adopt(context, metadata);
    identity::collect(&program.modules.iter().collect::<Vec<_>>(), context).unwrap();
    program
}

/// Recheck an edited MIR fixture with its retained definition scope.
pub fn verify_edited(
    context: &DefinitionContext,
    module: MirModule,
) -> Result<VerifiedMirModule, MirVerificationError> {
    let metadata =
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap();
    verify_mir(metadata, &Default::default())
}
