use kagari_embed::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_source::source::SourceFile;

use kagari_runtime::value::Value;

#[test]
fn standalone_language_examples_execute_from_source_and_artifact() {
    let cases: [(&str, &str, Value); 45] = [
        (
            "examples/syntax/callables.kgr",
            include_str!("../../../examples/syntax/callables.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/array-operations.kgr",
            include_str!("../../../examples/syntax/array-operations.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/bitwise.kgr",
            include_str!("../../../examples/syntax/bitwise.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/numeric-conversions.kgr",
            include_str!("../../../examples/syntax/numeric-conversions.kgr"),
            Value::I32(42),
        ),
        (
            "examples/6502-numeric.kgr",
            include_str!("../../../examples/6502-numeric.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/error-conversion.kgr",
            include_str!("../../../examples/syntax/error-conversion.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/type-inference.kgr",
            include_str!("../../../examples/syntax/type-inference.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/iterator-extensions.kgr",
            include_str!("../../../examples/syntax/iterator-extensions.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/collection-pipelines.kgr",
            include_str!("../../../examples/syntax/collection-pipelines.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/collection-access.kgr",
            include_str!("../../../examples/syntax/collection-access.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/string-interpolation.kgr",
            include_str!("../../../examples/syntax/string-interpolation.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/iterators.kgr",
            include_str!("../../../examples/syntax/iterators.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/conversions.kgr",
            include_str!("../../../examples/syntax/conversions.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/ordering.kgr",
            include_str!("../../../examples/syntax/ordering.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/operators.kgr",
            include_str!("../../../examples/syntax/operators.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/index.kgr",
            include_str!("../../../examples/syntax/index.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/standard-traits.kgr",
            include_str!("../../../examples/syntax/standard-traits.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/result-option.kgr",
            include_str!("../../../examples/syntax/result-option.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/generic-associated-types.kgr",
            include_str!("../../../examples/syntax/generic-associated-types.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/associated-constants.kgr",
            include_str!("../../../examples/syntax/associated-constants.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/default-methods.kgr",
            include_str!("../../../examples/syntax/default-methods.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/trait-inheritance.kgr",
            include_str!("../../../examples/syntax/trait-inheritance.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/generic-interfaces.kgr",
            include_str!("../../../examples/syntax/generic-interfaces.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/associated-types.kgr",
            include_str!("../../../examples/syntax/associated-types.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/visibility.kgr",
            include_str!("../../../examples/syntax/visibility.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/inline-modules.kgr",
            include_str!("../../../examples/syntax/inline-modules.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/tuple-types.kgr",
            include_str!("../../../examples/syntax/tuple-types.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/attributes.kgr",
            include_str!("../../../examples/syntax/attributes.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/binding-conditions.kgr",
            include_str!("../../../examples/syntax/binding-conditions.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/pattern-alternatives.kgr",
            include_str!("../../../examples/syntax/pattern-alternatives.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/match-guards.kgr",
            include_str!("../../../examples/syntax/match-guards.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/ranges.kgr",
            include_str!("../../../examples/syntax/ranges.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/blocks.kgr",
            include_str!("../../../examples/syntax/blocks.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/literals-and-comments.kgr",
            include_str!("../../../examples/syntax/literals-and-comments.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/closures.kgr",
            include_str!("../../../examples/syntax/closures.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/data-model.kgr",
            include_str!("../../../examples/syntax/data-model.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/expressions.kgr",
            include_str!("../../../examples/syntax/expressions.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/control-flow.kgr",
            include_str!("../../../examples/syntax/control-flow.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/for-collections.kgr",
            include_str!("../../../examples/syntax/for-collections.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/match.kgr",
            include_str!("../../../examples/syntax/match.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/where-bounds.kgr",
            include_str!("../../../examples/syntax/where-bounds.kgr"),
            Value::I32(42),
        ),
        (
            "examples/syntax/import-alias.kgr",
            include_str!("../../../examples/syntax/import-alias.kgr"),
            Value::I32(42),
        ),
        (
            "examples/interface-dispatch.kgr",
            include_str!("../../../examples/interface-dispatch.kgr"),
            Value::I32(42),
        ),
        (
            "examples/generic-trait-methods.kgr",
            include_str!("../../../examples/generic-trait-methods.kgr"),
            Value::I32(42),
        ),
        (
            "examples/standard-library.kgr",
            include_str!("../../../examples/standard-library.kgr"),
            Value::I32(42),
        ),
    ];

    let mut failures = Vec::new();
    for (path, source, expected) in cases {
        let engine = KagariEngine::default();
        let artifact =
            engine.compile_to_artifact(SourceFile::new(path, source), Default::default());
        let artifact = match artifact {
            Ok(artifact) => artifact,
            Err(error) => {
                failures.push(format!("{path} should compile: {error:?}"));
                continue;
            }
        };
        for encoded in [false, true] {
            let artifact = if encoded {
                BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
            } else {
                artifact.clone()
            };
            let context = ExecutionContext::default();
            let mut runtime = engine.runtime(context.clone());
            let loaded = runtime
                .load_program(
                    &PreparedProgram::from_artifact(
                        artifact,
                        &Default::default(),
                        &Default::default(),
                    )
                    .unwrap(),
                    Default::default(),
                )
                .unwrap_or_else(|error| panic!("{path} should load: {error:?}"));
            let actual = runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap_or_else(|error| panic!("{path} should execute: {error:?}"))
                .return_value;
            assert_eq!(actual, expected, "{path}, encoded={encoded}");
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn pattern_alternatives_require_the_same_bindings() {
    let engine = KagariEngine::default();
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
    let engine = KagariEngine::default();
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
fn grouped_standard_globs_execute() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "standard-glob.kgr",
                "use std::{collections::*}; fn main() -> i32 { var total=0;for value in map([22,20],|value|value){total+=value;}total }",
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
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn nested_inline_modules_resolve_qualified_members() {
    let engine = KagariEngine::default();
    let source = "mod outer { pub mod other { pub fn value() -> i32 { 42 } } pub mod inner { use super::other::value; pub fn call() -> i32 { value() } } } fn main() -> i32 { outer::inner::call() }";
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("nested-inline.kgr", source),
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
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn inline_module_errors_point_into_the_original_source() {
    let engine = KagariEngine::default();
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
fn for_loop_rejects_structural_change_and_cleans_up_after_trap() {
    let source = r#"
fn main() -> i32 {
    val values = [1, 2];
    for value in values {
        values.push(3);
    }
    0
}

fn after_trap() -> i32 { 42 }
"#;
    let engine = KagariEngine::default();
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
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert!(format!("{error:?}").contains("structural modification during iteration"));
    let result = runtime
        .execute(&loaded, "after_trap", &[], &context)
        .unwrap();
    assert_eq!(result.return_value, Value::I32(42));
}

#[test]
fn closure_trap_releases_execution_resources() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "closure_trap.kgr",
                r#"
fn fail() -> i32 {
    var count = 40;
    val fail = || { count = count + 1; 1 / 0 };
    fail()
}
fn after() -> i32 { 42 }
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
    assert!(runtime.execute(&loaded, "fail", &[], &context).is_err());
    assert_eq!(
        runtime
            .execute(&loaded, "after", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn remainder_matches_checked_arithmetic_contract() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "remainder.kgr",
                r#"
fn integer() -> i32 { 42 % 5 }
fn fractional() -> f32 { 5.5 % 2.0 }
fn zero() -> i32 { val divisor = 0; 5 % divisor }
fn overflow() -> i32 { val minimum = -2147483648; minimum % -1 }
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
            .return_value,
        Value::I32(2)
    );
    assert_eq!(
        runtime
            .execute(&loaded, "fractional", &[], &context)
            .unwrap()
            .return_value,
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
