use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("result-option.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let mut context = ExecutionContext::default();
        context.capabilities.jit = jit;
        context.language_profile.allow_jit = jit;
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(runtime.runtime().gc().stats().collections > 0);
    }
}

#[test]
fn constructors_patterns_and_nested_payloads_execute() {
    execute(
        r#"
fn option(x: i32)->Option<i32> { if x > 0 { Some(x) } else { None } }
fn result(x:i32)->Result<i32, String> { if x > 0 { Ok(x) } else { Err("negative") } }
fn main()->i32 {
    val a = match option(20) { Some(value) => value, None => 0, };
    val b = match result(22) { Ok(value) => value, Err(message) => 0, };
    val nested: Option<Result<i32, String>> = Option::Some(Result::Ok(a + b));
    match nested { Some(Ok(value)) => value, _ => 0, }
}
"#,
    );
}

#[test]
fn propagation_evaluates_once_and_returns_only_from_the_nearest_function() {
    execute(
        r#"
struct Counts { var calls: i32, var after: i32 }
fn read(c: Counts, pass: bool) -> Result<i32, String> {
    c.calls += 1;
    if pass { Ok(20) } else { Err("failed") }
}
fn forward(c: Counts, pass: bool) -> Result<String, String> {
    val value = read(c, pass)?;
    c.after += value;
    Ok("done")
}
fn missing() -> Option<i32> { None }
fn option() -> Option<String> { val value = missing()?; Some("unreachable") }
fn main() -> i32 {
    val c = Counts { calls: 0, after: 0 };
    val failed = forward(c, false);
    val good = forward(c, true);
    val callback: fn() -> Result<i32, String> = || { val x = read(c, false)?; Ok(x) };
    val nested = callback();
    val inferred = || { val x = read(c, true)?; Ok(x + 2) };
    val answer = inferred();
    if failed.is_err() && good.is_ok() && nested.is_err() && option().is_none() && c.calls == 4 && c.after == 20 {
        answer.unwrap_or(0) + c.after
    } else { 0 }
}
"#,
    );
}

#[test]
fn conversions_and_combinators_use_script_callbacks_lazily() {
    execute(
        r#"
struct Counter { var count: i32 }
fn error(c: Counter) -> String { c.count += 1; "missing" }
fn main() -> i32 {
    val c = Counter { count: 0 };
    val some: Option<i32> = Some(20);
    val none: Option<i32> = None;
    val eager = some.ok_or(error(c));
    val lazy = some.ok_or_else(|| error(c));
    val failed = none.ok_or_else(|| error(c));
    val converted = failed.map_err(|message| message.len_bytes());
    val mapped = some.map(|x| x + 1).and_then(|x| Some(x + 1));
    val result = eager.map(|x| x + 1).and_then(|x| Result<i32, String>::Ok(x + 1));
    if c.count == 2 && converted.is_err() && lazy.is_ok() && result.unwrap_or(0) == 22 {
        mapped.unwrap_or(0) + 20
    } else { 0 }
}
"#,
    );
}

#[test]
fn variant_imports_aliases_and_or_patterns_resolve_semantically() {
    execute(
        r#"
use std::option::{Some as Present, None as Absent};
use std::result::*;
fn main() -> i32 {
    val value: Option<i32> = Present(42);
    val absent: Option<i32> = Absent;
    val result: Result<i32, i32> = Err(42);
    val x = match result { Ok(n) | Err(n) => n, };
    if val Absent = absent { match value { Present(n) => n, Absent => 0, } } else { x - 42 }
}
"#,
    );
}

#[test]
fn invalid_propagation_constructors_and_callbacks_are_diagnosed() {
    for (source, code) in [
        (
            "fn f()->i32 { val x: Option<i32> = None; std::result::map_err(x, |n: i32| n); 42 }",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
        ),
        (
            "fn f()->i32 { val x: Option<i32> = None; x? }",
            "KG_TYPE_RETURN_TYPE_MISMATCH",
        ),
        (
            "fn f()->Result<i32, String> { val x: Option<i32> = None; Ok(x?) }",
            "KG_TYPE_RETURN_TYPE_MISMATCH",
        ),
        (
            "fn f()->Option<i32> { val x: Result<i32, String> = Err(\"x\"); Some(x?) }",
            "KG_TYPE_RETURN_TYPE_MISMATCH",
        ),
        (
            "fn f()->Result<i32, String> { val x: Result<i32, i32> = Err(1); Ok(x?) }",
            "KG_TYPE_RETURN_TYPE_MISMATCH",
        ),
        (
            "fn f()->Option<i32> { Some(42?) }",
            "KG_TYPE_RETURN_TYPE_MISMATCH",
        ),
        (
            "fn f()->Option<i32> { Some(\"bad\") }",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
        ),
        (
            "fn f()->i32 { val x = None; 42 }",
            "KG_TYPE_CANNOT_INFER_GENERIC_ARGUMENT",
        ),
        (
            "fn f()->Option<i32> { None() }",
            "KG_TYPE_INVALID_CALL_TARGET",
        ),
        (
            "fn f()->i32 { val x: Option<i32> = Some(1); val y = x.ok_or_else(|x: i32| x); 42 }",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
        ),
        (
            "fn f()->i32 { val x: Option<i32> = Some(1); val y = x.and_then(|n| n); 42 }",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
        ),
        (
            "fn f()->i32 { val x: Option<i32> = Some(1); match x { Some => 42, _ => 0, } }",
            "KG_TYPE_PATTERN_MISMATCH",
        ),
        (
            "fn f()->Option<i32> { val cb = || { val x: Option<i32> = None; x?; 42 }; Some(cb()) }",
            "KG_TYPE_RETURN_TYPE_MISMATCH",
        ),
    ] {
        let error = KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new("invalid.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap_err();
        let kagari_embed::EmbeddingError::Diagnostics { diagnostics } = error else {
            panic!("{source}: {error:?}");
        };
        assert!(
            diagnostics.iter().any(|d| d.code == code),
            "{source}: expected {code}, got {diagnostics:?}"
        );
    }
}

#[test]
fn generic_propagation_nested_question_marks_and_loop_cleanup() {
    execute(
        r#"
fn identity<T, E>(value: Result<T, E>) -> Result<T, E> { Ok(value?) }
fn leave(values: MutableArray<i32>) -> Option<i32> {
    for value in values { val absent: Option<i32> = None; absent?; }
    Some(0)
}
fn nested(value: Option<Option<i32>>) -> Option<i32> { Some(value??) }
fn main()->i32 {
    val values = [1];
    val left = leave(values);
    values.push(2);
    val x: Result<i32, String> = Ok(20);
    val y: Option<Option<i32>> = Some(Some(22));
    if left.is_none() && values.len() == [1, 2].len() { identity(x).unwrap_or(0) + nested(y).unwrap_or(0) } else { 0 }
}
"#,
    );
}

#[test]
fn explicit_error_conversion_and_propagation_preserve_payload_identity() {
    execute(
        r#"
struct ErrorInfo { var code: i32 }
fn fail(error: ErrorInfo)->Result<i32, ErrorInfo> { Err(error) }
fn forward(error: ErrorInfo)->Result<String, ErrorInfo> { fail(error)?; Ok("never") }
fn translated()->Result<i32, String> { val x: Result<i32, i32> = Err(7); Ok(x.map_err(|code| "converted")?) }
fn main()->i32 {
    val error = ErrorInfo { code: 0 };
    val returned = forward(error);
    if val Err(original) = returned { original.code = 42; }
    if translated().is_err() { error.code } else { 0 }
}
"#,
    );
}

#[test]
fn malformed_standard_enum_operations_are_rejected_before_execution() {
    use kagari_ir::{
        bytecode::BytecodeInstruction,
        module::{
            abi::{AbiType, BuiltinType, StandardEnumKind},
            instruction::StandardEnumOp,
        },
    };
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new("verified.kgr", "fn main()->Option<i32> { Some(42) }"),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for case in 0..5 {
        let mut program = artifact.program.clone();
        let instruction = program
            .modules
            .iter_mut()
            .flat_map(|m| &mut m.functions)
            .flat_map(|f| &mut f.instructions)
            .find(|i| matches!(i, BytecodeInstruction::StandardEnum { .. }))
            .unwrap();
        let BytecodeInstruction::StandardEnum { op, ty, value, .. } = instruction else {
            unreachable!()
        };
        match case {
            0 => *op = StandardEnumOp::Make(9),
            1 => {
                *ty = AbiType::StandardEnum {
                    kind: StandardEnumKind::Result,
                    args: vec![AbiType::Builtin(BuiltinType::I32)],
                }
            }
            2 => *value = None,
            3 => {
                *ty = AbiType::StandardEnum {
                    kind: StandardEnumKind::Option,
                    args: vec![AbiType::StandardEnum {
                        kind: StandardEnumKind::Result,
                        args: vec![],
                    }],
                }
            }
            _ => *op = StandardEnumOp::Read(1),
        }
        assert!(
            BytecodeArtifact::from_program(program, Default::default()).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn propagation_does_not_catch_traps_or_termination_and_releases_frames() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "cleanup.kgr",
                r#"
fn fail()->Result<i32, String> { val zero = 0; Ok(42 / zero) }
fn main()->Result<i32, String> { for x in [1, 2] { fail()?; } Ok(42) }
fn after()->i32 { 42 }
"#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let error = runtime
        .execute(&loaded, "main", &[], &ExecutionContext::default())
        .unwrap_err();
    assert!(format!("{error:?}").contains("division by zero"));
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    let mut limited = ExecutionContext::default();
    limited.resources.max_instruction_steps = Some(2);
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &limited)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    let cancelled = ExecutionContext::default();
    cancelled.cancellation.cancel();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &cancelled)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime
            .execute(&loaded, "after", &[], &ExecutionContext::default())
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert!(!runtime.runtime().is_quarantined());
}

#[test]
fn payload_early_returns_and_closure_residual_inference_compose() {
    execute(
        r#"
fn early()->Option<i32> { Some({ return Some(42); }) }
fn result()->Result<i32, String> { Err("skip") }
fn main()->i32 {
    val present: Option<i32> = Some(1);
    val mapped = present.map(|n| { result()?; Ok(n) });
    if val Some(Err(message)) = mapped { early().unwrap_or(0) } else { 0 }
}
"#,
    );
}
