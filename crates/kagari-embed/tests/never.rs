mod support;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
};
use kagari_runtime::{host::HostFunction, value::Value};
use std::sync::{Arc, Mutex};
use {kagari_common::host_interface::standard_log, kagari_source::source::SourceFile};

fn execute(source: &str, entry: &str, expected: i32) {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(SourceFile::new("never.kgr", source), Default::default())
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
        let prepared =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let native = runtime
                .prepare_native(
                    &prepared,
                    &loaded,
                    entry,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, entry, &[], &context, &native)
        } else {
            runtime.execute(&loaded, entry, &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(expected));
    }
}

#[test]
fn never_calls_branches_and_generic_arguments() {
    execute(
        r#"
fn fail() -> ! { val zero = 0; 1 / zero; loop {} }
fn forever() -> ! { loop {} }
fn absurd(x: !) -> i32 { x }
fn absurd_generic<T>(x: !) -> T { x }
fn impossible_branch(x: !) -> i32 { if true { x } else { 1 } }
fn generic_fail<T>() -> T { fail() }
fn invoke<T>(callback: fn() -> T) -> T { callback() }
fn choose(ok: bool) -> i32 { if ok { 42 } else { fail() } }
fn success<T>(value: T) -> Result<T, !> { Ok(value) }
fn main() -> i32 {
    val value: Result<i32, !> = success(choose(true));
    match value { Ok(value) => value, Err(error) => match error {} }
}

"#,
        "main",
        42,
    );
}

#[test]
fn rejects_normal_returns_and_nested_never_coercions() {
    let engine = KagariEngine::default();
    for source in [
        "fn wrong() -> ! {}",
        "fn wrong() -> ! { return; }",
        "fn wrong() -> ! { 42 }",
        "fn wrong() -> ! { return 42; }",
        "fn wrong() -> ! { loop { break; } }",
        "fn wrong(x: Vec<!>) -> Vec<i32> { x }",
        "fn wrong(x: Vec<!>) { x[0usize] = 1; }",
        "fn wrong(value: !) { var x: ! = value; x = 1; }",
        "fn wrong(x: fn() -> !) -> fn() -> i32 { x }",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("never-invalid.kgr", source),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn never_closures_and_loop_joins() {
    execute(
        r#"
fn fail() -> ! { val zero = 0; 1 / zero; loop {} }
fn invoke<T>(callback: fn() -> T) -> T { callback() }
fn accept(callback: fn() -> !) -> ! { callback() }
fn inferred_argument() { invoke(|| fail()); }
fn inferred_return() { val stop = || { return fail(); }; stop(); }
fn select<T>(callback: fn() -> T, value: T) -> T { value }
fn main() -> i32 {
    val stop = || fail();
    val value = if false { invoke(stop) } else { 20 };
    val contextual = select(|| fail(), value);
    val next = loop { if true { break 22; } else { fail(); } };
    contextual + next
}
"#,
        "main",
        42,
    );
}

#[test]
fn never_preserves_effects_and_releases_resources_on_traps_and_cancellation() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "never-effects.kgr",
                r#"
fn fail(values: Vec<i32>) -> ! { values.push(20); print("before"); val zero = 0; 1 / zero; loop {} }
fn later(values: Vec<i32>) -> i32 { print("after"); values.push(99); 99 }
fn consume(a: i32, b: i32) -> i32 { a + b }
fn main() -> i32 { val values = [1]; consume(fail(values), later(values)) }
fn forever() -> ! { loop {} }
fn impossible(value: !) -> i32 { value }
fn healthy() -> i32 { 42 }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for encoded in [false, true] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext::default();

        let mut runtime = engine.runtime(context.clone());
        let prepared =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let effects = Arc::new(Mutex::new(Vec::new()));
        let captured = effects.clone();
        runtime
            .runtime_mut()
            .register_host_function(HostFunction::new(standard_log(), move |_, args| {
                captured.lock().unwrap().extend_from_slice(args);
                Ok(Value::Unit)
            }))
            .unwrap();
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
        assert_eq!(*effects.lock().unwrap(), vec![Value::Str("before".into())]);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(runtime.runtime().execution_root().is_none());
        let limited = context.clone();

        let cancellation = support::cancel_after(runtime.runtime(), &loaded, 20);
        let error = runtime
            .execute(&loaded, "forever", &[], &limited)
            .unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_CANCELLED");
        drop(cancellation);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(runtime.runtime().execution_root().is_none());

        assert_eq!(
            runtime
                .execute(&loaded, "healthy", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}

#[test]
fn never_survives_trait_dispatch_and_callable_adapters() {
    execute(
        r#"
trait Halt { fn stop(self) -> !; }
struct Stop {}
impl Halt for Stop { fn stop(self) -> ! { val zero = 0; 1 / zero; loop {} } }
impl Fn<()> for Stop {
    type Output = !;
    fn call(self, args: ()) -> ! { val zero = 0; 1 / zero; loop {} }
}
fn via_interface(value: Halt) -> ! { value.stop() }
fn via_callback(callback: fn() -> !) -> ! { callback() }
fn main() -> i32 {
    if false { via_interface(Stop {}) }
    else if false { via_callback(Stop {}) }
    else { 42 }
}
"#,
        "main",
        42,
    );
}

#[test]
fn never_containers_and_short_circuiting_keep_normal_paths() {
    execute(
        r#"use std::collections::{List};

fn fail() -> ! { val zero = 0; 1 / zero; loop {} }
fn main() -> i32 {
    val values: Vec<!> = [];
    val readonly: List<!> = values;
    val missing: Option<!> = None;
    val result: Result<!, i32> = Err(42);
    val copied: Result<!, i32> = match result { Ok(value) => value, Err(error) => Err(error) };
    val unused = false && fail();
    val skipped = true || fail();
    if !(readonly.len() == 0usize && (match missing { None => true, Some(value) => value }) && skipped && !unused) { return 0; }
    match copied { Ok(value) => value, Err(error) => error }
}
"#,
        "main",
        42,
    );
}
