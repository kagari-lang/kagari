use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

#[test]
fn standalone_language_examples_execute_from_source_and_artifact() {
    let cases: [(&str, &str, Value); 10] = [
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
            Value::Tuple(vec![
                Value::I64(3),
                Value::Bool(true),
                Value::I64(2),
                Value::Bool(true),
                Value::I64(2),
                Value::Bool(true),
                Value::I32(12),
            ]),
        ),
    ];

    for (path, source, expected) in cases {
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(path, source),
                Default::default(),
                Default::default(),
            )
            .unwrap_or_else(|error| panic!("{path} should compile: {error:?}"));
        for encoded in [false, true] {
            let artifact = if encoded {
                BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
            } else {
                artifact.clone()
            };
            let context = ExecutionContext::default();
            let mut runtime = engine.runtime(context.clone());
            let loaded = runtime
                .load_program(artifact, Default::default())
                .unwrap_or_else(|error| panic!("{path} should load: {error:?}"));
            let actual = runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap_or_else(|error| panic!("{path} should execute: {error:?}"))
                .return_value;
            assert_eq!(actual, expected, "{path}, encoded={encoded}");
        }
    }
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
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert!(format!("{error:?}").contains("structural modification during iteration"));
    let result = runtime
        .execute(&loaded, "after_trap", &[], &context)
        .unwrap();
    assert_eq!(result.return_value, Value::I32(42));
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
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
