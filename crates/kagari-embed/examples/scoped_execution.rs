//! Per-call budgets and cooperative cancellation without changing runtime defaults.
use kagari_common::SourceFile;
use kagari_embed::{ArtifactOptions, CompileOptions, ExecutionContext, KagariEngine, LoadOptions};
use kagari_runtime::value::Value;

fn main() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("scoped.kgr", "fn main() -> i32 { 42 }"),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let loaded = runtime
        .load_program(artifact, LoadOptions::default())
        .unwrap();
    assert!(
        runtime
            .execute(&loaded, "missing", &[], &ExecutionContext::default())
            .is_err()
    );
    assert_eq!(
        runtime.runtime().resources().counters().instruction_steps,
        0
    );
    println!("missing entry was rejected before any initializer instruction");
    let mut context = ExecutionContext::default();
    context.resources.max_instruction_steps = Some(2);
    for _ in 0..2 {
        let report = runtime.execute(&loaded, "main", &[], &context).unwrap();
        assert_eq!(report.return_value, Value::I32(42));
    }
    assert_eq!(
        runtime.runtime().resources().counters().instruction_steps,
        4
    );
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
    println!("two independent budgets completed; cancellation left the runtime reusable");
    // A crafted product with two equally named entry functions must not pick one.
    let ambiguous = engine
        .compile_to_artifact(
            SourceFile::new(
                "ambiguous.kgr",
                "fn main() -> i32 { 42 } fn alternative() -> i32 { 43 }",
            ),
            CompileOptions::default(),
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
    let ambiguous =
        kagari_embed::BytecodeArtifact::from_program(program, Default::default()).unwrap();
    let loaded = runtime
        .load_program(ambiguous, LoadOptions::default())
        .unwrap();
    let before = runtime.runtime().resources().counters().instruction_steps;
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &fresh)
            .unwrap_err()
            .code(),
        "KG_BYTECODE_VERIFICATION_FAILED"
    );
    assert_eq!(
        runtime.runtime().resources().counters().instruction_steps,
        before
    );
    println!("ambiguous entry was rejected before script execution");
}
