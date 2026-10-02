mod support;
use kagari_common::source::SourceFile;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
    program::PreparedProgram,
};

use kagari_runtime::value::Value;

#[test]
fn propagation_requires_one_infallible_conversion_bound() {
    for source in [
        "fn forward<T,E,F>(value:Result<T,E>)->Result<T,F>{Ok(value?)} fn main(){}",
        "struct E{} struct F{} impl F {fn convert(e:E)->Result<Self,String>{Ok(F{})}} fn main()->Result<i32,F>{val x:Result<i32,E> = Err(E{});Ok(x?)}",
        "struct E{} struct Mid{} struct F{} impl From<E> for Mid {fn from(e:E)->Self{Mid{}}} impl From<Mid> for F {fn from(e:Mid)->Self{F{}}} fn main()->Result<i32,F>{val x:Result<i32,E> = Err(E{});Ok(x?)}",
    ] {
        let error = KagariEngine::default()
            .compile_source(SourceFile::new("missing-error-conversion.kgr", source))
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
            "{error:?}"
        );
    }
}

#[test]
fn traps_inside_error_conversion_release_resources() {
    let source = r#"
struct Source { var calls: i32 }
struct Target {}
impl From<Source> for Target {
    fn from(error: Source) -> Self {
        error.calls += 1;
        val zero=0; 1/zero;
        Target {}
    }
}
fn fail(error: Source) -> Result<i32, Target> {
    val value: Result<i32, Source> = Err(error);
    Ok(value?)
}
fn main() -> Result<i32, Target> { fail(Source { calls: 0 }) }
fn after() -> i32 { 42 }
"#;
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("conversion-trap.kgr", source),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert!(
        format!("{error:?}").contains("division by zero"),
        "{error:?}"
    );
    assert!(
        error
            .error_trace()
            .unwrap()
            .frames
            .iter()
            .any(|frame| frame.function_name.contains("from"))
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "after", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("result-option.kgr", source),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let result = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
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
fn propagation_converts_only_errors_through_from_bounds() {
    execute(
        r#"
struct SourceError { var reads: i32, var conversions: i32 }
struct AppError { val cause: SourceError }
impl From<SourceError> for AppError {
    fn from(error: SourceError) -> Self {
        error.conversions += 1;
        AppError { cause: error }
    }
}
fn read(error: SourceError, fail: bool) -> Result<i32, SourceError> {
    error.reads += 1;
    if fail { Err(error) } else { Ok(42) }
}
fn forward<T, E, F: From<E>>(value: Result<T, E>) -> Result<T, F> { Ok(value?) }
fn inferred_success() -> Result<i32, AppError> { Ok(Ok(42)?) }
fn direct(error: SourceError) -> Result<i32, AppError> {
    val value: i32 = Err(error)?;
    Ok(value)
}
fn main() -> i32 {
    val error = SourceError { reads: 0, conversions: 0 };
    val failure: Result<i32, AppError> = forward(read(error, true));
    val success: Result<i32, AppError> = forward(read(error, false));
    if error.reads != 2 || error.conversions != 1 {return 0;}
    val same = match failure {Err(e)=>e.cause === error,Ok(_)=>false};
    if !same {return 0;}
    val callback: fn() -> Result<i32, AppError> = || { Ok(read(error, true)?) };
    if val Ok(_) = callback() {return 0;}
    if val Ok(_) = direct(error) {return 0;}
    if error.conversions != 3 {return 0;}
    val inferred = match inferred_success() {Ok(n)=>n,Err(_)=>0};
    if inferred != 42 {return 0;}
    match success {Ok(n)=>n,Err(_)=>0}
}
"#,
    );
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
    val correct = match failed {Err(_)=>match good {Ok(_)=>match nested {Err(_)=>match option(){None=>true,Some(_)=>false},Ok(_)=>false},Err(_)=>false},Ok(_)=>false};
    if correct && c.calls == 4 && c.after == 20 {
        (match answer {Ok(n)=>n,Err(_)=>0}) + c.after
    } else { 0 }
}
"#,
    );
}

#[test]
fn variant_imports_aliases_and_or_patterns_resolve_semantically() {
    execute(
        r#"
use core::language::Option::{Some as Present, None as Absent};
use core::language::Result::{Ok, Err};
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
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
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
            "fn f()->i32 { val x: Option<i32> = Some(1); match x { Some => 42, _ => 0, } }",
            "KG_TYPE_PATTERN_MISMATCH",
        ),
        (
            "fn f()->Option<i32> { val cb = || { val x: Option<i32> = None; x?; 42 }; Some(cb()) }",
            "KG_TYPE_RETURN_TYPE_MISMATCH",
        ),
    ] {
        let error = KagariEngine::default()
            .compile_to_artifact(SourceFile::new("invalid.kgr", source), Default::default())
            .unwrap_err();
        let EmbeddingError::Diagnostics { diagnostics } = error else {
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
fn leave(values: ArrayList<i32>) -> Option<i32> {
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
    val absent=match left {None=>true,Some(_)=>false};
    if absent && values.len() == [1,2].len() {(match identity(x){Ok(n)=>n,Err(_)=>0}) + (match nested(y){Some(n)=>n,None=>0})}else{0}
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
struct Converted {val code:i32}
impl From<i32> for Converted {fn from(code:i32)->Self{Converted{code}}}
fn translated()->Result<i32, Converted> { val x: Result<i32, i32> = Err(7); Ok(x?) }
fn main()->i32 {
    val error = ErrorInfo { code: 0 };
    val returned = forward(error);
    if val Err(original) = returned { original.code = 42; }
    match translated(){Err(e)=>if e.code==7 {error.code}else{0},Ok(_)=>0}
}
"#,
    );
}

#[test]
fn malformed_standard_enum_operations_are_rejected_before_execution() {
    use kagari_abi::{
        operations::StandardEnumOp, scalar::BuiltinType,
        standard::surface::StandardEnum as StandardEnumKind, types::AbiType,
    };
    use kagari_bytecode::instruction::BytecodeInstruction;
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new("verified.kgr", "fn main()->Option<i32> { Some(42) }"),
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
        )
        .unwrap();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    let error = runtime
        .execute(&loaded, "main", &[], &ExecutionContext::default())
        .unwrap_err();
    assert!(format!("{error:?}").contains("division by zero"));
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    let limited = ExecutionContext::default();
    let cancellation = support::cancel_after(runtime.runtime(), &loaded, 1);

    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &limited)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    drop(cancellation);
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
fn apply<T,R>(value:T,callback:fn(T)->R)->R{callback(value)}
fn main()->i32 {
    val mapped = apply(1, |n| { result()?; Ok(n) });
    if val Err(message) = mapped {match early(){Some(n)=>n,None=>0}}else{0}
}
"#,
    );
}
