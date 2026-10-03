//! Independent execution sessions and cooperative cancellation.

use kagari_embed::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    program::PreparedProgram,
    runtime::LoadOptions,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;

fn main() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("scoped.kgr", "fn main() -> i32 { 42 }"),
            ArtifactOptions::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
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
        let report = runtime.execute(&loaded, "main", &[], &context).unwrap();
        assert_eq!(report.return_value, Value::I32(42));
    }

    // A host can keep a clone of this token to request cancellation.
    context.cancellation.cancel();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    let fresh = ExecutionContext::default();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &fresh)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    println!(
        "two independent execution sessions completed; cancellation left the runtime reusable"
    );
    // A crafted product with two equally named entry functions must not pick one.
    let ambiguous = engine
        .compile_to_artifact(
            SourceFile::new(
                "ambiguous.kgr",
                "fn main() -> i32 { 42 } fn alternative() -> i32 { 43 }",
            ),
            ArtifactOptions::default(),
        )
        .unwrap();
    let mut program = ambiguous.program;
    let root = program.root.index();
    let alternative = program.modules[root]
        .functions
        .iter()
        .position(|function| function.name == "alternative")
        .unwrap();
    program.modules[root].functions[alternative].name = "main".into();
    program.modules[root].function_table[alternative].name = "main".into();
    let ambiguous = BytecodeArtifact::from_program(program, Default::default()).unwrap();
    let program =
        PreparedProgram::from_artifact(ambiguous, &Default::default(), &Default::default())
            .unwrap();
    let loaded = runtime
        .load_program(&program, LoadOptions::default())
        .unwrap();

    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &fresh)
            .unwrap_err()
            .code(),
        "KG_BYTECODE_VERIFICATION_FAILED"
    );

    println!("ambiguous entry was rejected before script execution");
}
