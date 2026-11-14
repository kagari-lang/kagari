//! Independent execution sessions and cooperative cancellation.

use kagari::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    program::Program,
    runtime::LoadOptions,
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;

fn main() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = engine
        .compile_to_program(
            SourceFile::new("scoped.kgr", "fn main() -> i32 { 42 }"),
            Default::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let loaded = runtime
        .load_program(&program, LoadOptions::default())
        .unwrap();
    assert!(
        runtime
            .execute(&loaded, "missing", &[], &ExecutionContext::default())
            .is_err()
    );

    println!("missing entry was rejected before any initializer instruction");
    let context = ExecutionContext::default();

    for _ in 0..2 {
        let report = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap();
        assert_eq!(report.return_value.as_i32(), Some(42));
    }

    // A host can keep a clone of this token to request cancellation.
    context.cancellation.cancel();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    let fresh = ExecutionContext::default();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &fresh)
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    println!(
        "two independent execution sessions completed; cancellation left the runtime reusable"
    );
    // A crafted product with two equally named entry functions must not pick one.
    let relabeled = engine
        .compile_to_artifact(
            SourceFile::new(
                "relabeled.kgr",
                "fn main() -> i32 { 42 } fn alternative() -> i32 { 43 }",
            ),
            ArtifactOptions::default(),
        )
        .unwrap();
    let definitions = DefinitionContext::new().unwrap();
    let relabeled = relabeled
        .import_into(&definitions, &Default::default())
        .unwrap();
    let mut program = relabeled.into_records().program;
    let root = program.root.index();
    let alternative = program.modules[root]
        .functions
        .iter()
        .position(|function| function.name == "alternative")
        .unwrap();
    program.modules[root].functions[alternative].name = "main".into();
    program.modules[root].function_table[alternative].name = "main".into();
    let relabeled = BytecodeArtifact::from_program(
        program,
        Default::default(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    let relabeled =
        DefinitionMetadata::checked(definitions.snapshot(), relabeled, &Default::default())
            .unwrap();
    let program =
        Program::from_artifact(relabeled, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&program, LoadOptions::default())
        .unwrap();

    // Execution uses the verified entry capability, not these display labels.
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &fresh)
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    println!("display labels leave the verified entry selection unchanged");
}
