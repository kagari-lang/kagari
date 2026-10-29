use crate::{
    BytecodeArtifact, context::ExecutionContext, error::EmbeddingError, program::PreparedProgram,
};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;
use rstest::rstest;
use std::{fs, path::PathBuf};

use kagari_runtime::value::Value;

use crate::test_support::execution;

#[rstest]
fn source_examples(
    #[files("../../examples/syntax/*.kgr")]
    #[exclude(r"[/\\](async|byte-literals|char-literals|grammar-witnesses|try-protocols)\.kgr$")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("failed to read syntax example {}: {error}", path.display())
    });
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 syntax example file name");
    execute_source_example(
        &format!("examples/syntax/{filename}"),
        &source,
        &[execution::Route::Source, execution::Route::Artifact],
    );
}

#[rstest]
#[case::numeric_6502_numeric(
    "examples/6502-numeric.kgr",
    include_str!("../../../../../../examples/6502-numeric.kgr")
)]
#[case::interface_dispatch(
    "examples/interface-dispatch.kgr",
    include_str!("../../../../../../examples/interface-dispatch.kgr")
)]
#[case::generic_trait_methods(
    "examples/generic-trait-methods.kgr",
    include_str!("../../../../../../examples/generic-trait-methods.kgr")
)]
#[case::standard_library(
    "examples/standard-library.kgr",
    include_str!("../../../../../../examples/standard-library.kgr")
)]
#[case::struct_pattern_rest_executes_in_existing_contexts(
    "struct-pattern-rest.kgr",
    include_str!("../../../../tests/fixtures/test-fixtures/language/struct-pattern-rest.kgr")
)]
#[case::grouped_standard_globs_execute(
    "standard-glob.kgr",
    include_str!("../../../../tests/fixtures/test-fixtures/language/standard-glob.kgr"),
    &[execution::Route::Artifact]
)]
fn additional_source_examples(
    #[case] path: &str,
    #[case] source: &str,
    #[case(default(&[execution::Route::Source, execution::Route::Artifact]))]
    routes: &[execution::Route],
) {
    execute_source_example(path, source, routes);
}

fn execute_source_example(path: &str, source: &str, routes: &[execution::Route]) {
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_source(SourceFile::new(path, source))
        .unwrap();
    execution::assert_returns_i32(&engine, artifact, routes, 42);
}

#[test]
fn pattern_alternatives_require_the_same_bindings() {
    let engine = execution::engine(Default::default());
    let error = engine
        .compile_to_artifact(
            SourceFile::new(
                "bad-or-pattern.kgr",
                "fn main() -> i32 { match (1, 2) { (1, x) | (2, y) => x, _ => 0 } }",
            ),
            Default::default(),
        )
        .unwrap_err();
    assert!(format!("{error:?}").contains("the same bindings in every alternative"));
}

#[test]
fn attributes_without_compiler_behavior_are_rejected_before_execution() {
    let engine = execution::engine(Default::default());
    for (attribute, expected) in [
        ("#[requires(role = \"admin\")]", "KG_ATTRIBUTE_UNSUPPORTED"),
        ("#[unregistered]", "KG_ATTRIBUTE_UNKNOWN"),
    ] {
        let source = format!("{attribute} fn main() -> i32 {{ 42 }}");
        let error = engine
            .compile_to_artifact(
                SourceFile::new("attribute-rejection.kgr", source),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            format!("{error:?}").contains(expected),
            "{attribute}: {error:?}"
        );
    }
}

#[test]
fn nested_inline_modules_resolve_qualified_members() {
    let engine = execution::engine(Default::default());
    let source = "pub mod outer { pub mod other { pub fn value() -> i32 { 14 } } pub mod inner { use super::other::value; pub fn call() -> i32 { value() } } } mod facade { pub use super::outer as api; pub use super::outer::other::value as answer; } use self::facade::{api as m, answer as direct}; fn main() -> i32 { m::other::value() + direct() + m::inner::call() }";
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("nested-inline.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    drop(engine);
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn inline_module_errors_point_into_the_original_source() {
    let engine = execution::engine(Default::default());
    let id = engine
        .set_source(
            "mem://inline-error",
            "mod child { pub fn value() -> i32 { missing } } fn main() -> i32 { child::value() }"
                .into(),
            kagari_source::source_database::SourceLayer::Base,
        )
        .unwrap();
    let error = engine
        .compile_snapshot(engine.source_snapshot(), id, &Default::default())
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected diagnostics");
    };
    assert!(
        diagnostics.iter().any(
            |diagnostic| diagnostic.span.is_some_and(|span| span.file == id
                && span.range.start >= "mod child { pub fn value() -> i32 { ".len())
        ),
        "{diagnostics:?}"
    );
}

#[test]
fn for_loop_detects_structural_change_and_cleans_up_after_trap() {
    let source = r#"
fn main() -> i32 {
    val values = Vec::from([1, 2]);
    for value in values {
        values.push(3);
    }
    0
}

pub fn after_trap() -> i32 { 42 }
"#;
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("for-mutation.kgr", source),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_err();
    assert!(format!("{error:?}").contains("invalid or structurally modified iterator"));
    let result = runtime
        .execute(&loaded, "after_trap", &[], &context)
        .unwrap();
    assert_eq!(
        result
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn closure_trap_releases_execution_resources() {
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "closure_trap.kgr",
                include_str!("../../../../tests/fixtures/test-fixtures/language/closure-trap.kgr"),
            ),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert!(runtime.execute(&loaded, "fail", &[], &context).is_err());
    assert_eq!(
        runtime
            .execute(&loaded, "after", &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn remainder_matches_checked_arithmetic_contract() {
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "remainder.kgr",
                r#"
pub fn integer() -> i32 { 42 % 5 }
pub fn fractional() -> f32 { 5.5 % 2.0 }
pub fn zero() -> i32 { val divisor = 0; 5 % divisor }
pub fn overflow() -> i32 { val minimum = -2147483648; minimum % -1 }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "integer", &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(2)
    );
    assert_eq!(
        runtime
            .execute(&loaded, "fractional", &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_f32().map(Value::F32))
            .expect("retained execution result"),
        Value::F32(1.5)
    );
    for (name, message) in [
        ("zero", "integer remainder by zero"),
        ("overflow", "integer overflow"),
    ] {
        let error = runtime.execute(&loaded, name, &[], &context).unwrap_err();
        assert!(format!("{error:?}").contains(message), "{name}: {error:?}");
    }
}
