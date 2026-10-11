mod support;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{host::HostFunction, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::host_interface::standard_log;
use std::sync::{Arc, Mutex};

fn execute(source: &str, entry: &str, expected: i32) {
    execute_entries(source, &[(entry, expected)]);
}

fn execute_entries(source: &str, entries: &[(&str, i32)]) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
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
        for &(entry, expected) in entries {
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
            assert_eq!(
                result
                    .return_value
                    .value(runtime.runtime().gc())
                    .expect("retained execution result"),
                Value::I32(expected),
                "{entry}, encoded={encoded}, jit={jit}"
            );
            drop(result);
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
        }
    }
}

// One source program per contract group, with an independently observed entry for
// each evaluation position. `STOP` increments the counter once before returning;
// writes or later operands would change the entry's expected result.
fn returning_expressions(cases: &[(&str, &str, i32)]) {
    let mut source = String::from(
        r#"
use std::collections;
struct Count { var value: i32 }
struct Item<T> { val value: T, val flag: bool }
enum Packet<T> { Value(T, bool) }
trait Take { fn take(self, input: bool) -> i32; }
impl Take for Count { fn take(self, input: bool) -> i32 { self.value += 100; 0 } }
fn tick(count: Count) -> bool { count.value += 1; true }
fn later(count: Count) -> i32 { count.value += 100; 99 }
fn take(count: Count, value: i32) -> i32 { count.value += 100; value }
fn generic_take<T: SignedNumber>(count: Count, value: T) -> i32 { count.value += 100; 0 }
fn bound_take<T: Take>(value: T, count: Count) -> i32 {
    value.take(if tick(count) { return 40; } else { return 0; })
}
fn grow<T>(value: T) -> i32 { grow((value, value)) }
fn index(value: ()) -> i32 { 0 }
fn matrix(value: ()) -> [[i32]] { [[0]] }
"#,
    );
    for &(name, body, _) in cases {
        let body = body.replace("STOP", "(if tick(count) { return 40; } else { return 0; })");
        source.push_str(&format!(
            "fn run_{name}(count: Count, array: [i32]) -> i32 {{
                var tuple = (1, true); {body}
             }}
             fn {name}() -> i32 {{
                val count = Count {{ value: 1 }}; val array = [10];
                run_{name}(count, array) + count.value + array[0]
             }}\n"
        ));
    }
    let entries: Vec<_> = cases
        .iter()
        .map(|&(name, _, expected)| (name, expected))
        .collect();
    execute_entries(&source, &entries);
}

#[test]
fn returning_expressions_preserve_control_flow_and_short_circuiting() {
    returning_expressions(&[
        ("nested_return", "return STOP;", 52),
        ("if_condition", "if STOP { tick(count); } else { 7 }; 0", 52),
        ("while_condition", "while STOP { tick(count); } 0", 52),
        (
            "match_scrutinee",
            "match STOP { 1 => tick(count), _ => 7 }; 0",
            52,
        ),
        ("negation", "-STOP; 0", 52),
        ("not", "!STOP; 0", 52),
        ("binary_left", "STOP + later(count); 0", 52),
        ("binary_right", "later(count) + STOP; 0", 152),
        ("comparison", "STOP < later(count); 0", 52),
        ("equality", "STOP == false; 0", 52),
        ("and_taken", "true && STOP; 0", 52),
        ("or_taken", "false || STOP; 0", 52),
        ("and_skipped", "false && STOP; 0", 11),
        ("or_skipped", "true || STOP; 0", 11),
        ("helper_skipped", "false && (type_of(STOP) == \"\"); 0", 11),
    ]);
}

#[test]
fn returning_expressions_stop_calls_and_generic_instantiation() {
    returning_expressions(&[
        ("function_argument", "take(count, STOP)", 52),
        ("generic_argument", "generic_take(count, STOP)", 52),
        ("bound_argument", "bound_take(count, count)", 52),
        ("helper_argument", "type_of(STOP); 0", 52),
        ("native_receiver", "STOP.sort(); 0", 52),
        ("native_argument", "Vec::from([1]).sort_by(STOP); 0", 52),
        (
            "native_source",
            "collections::map(STOP, |n:i32| { tick(count); n }); 0",
            52,
        ),
        ("callee", "STOP(tick(count)); 0", 52),
        ("member_callee", "STOP.missing(tick(count)); 0", 52),
        ("unreachable_instance", "take(count, STOP) + grow(1)", 52),
    ]);
}

#[test]
fn returning_expressions_stop_aggregate_construction() {
    returning_expressions(&[
        (
            "array_element",
            "val values = [later(count), STOP, true, later(count)]; 0",
            152,
        ),
        ("tuple_element", "(later(count), STOP, tick(count)); 0", 152),
        ("enum_payload", "Packet::Value(STOP, tick(count)); 0", 52),
        (
            "struct_field",
            "Item { value: STOP, flag: tick(count) }; 0",
            52,
        ),
    ]);
}

#[test]
fn returning_expressions_skip_accesses_and_uncommitted_writes() {
    returning_expressions(&[
        ("initializer", "val value: i32 = STOP; later(count)", 52),
        ("assignment", "count.value = STOP; 0", 52),
        ("compound_value", "count.value += STOP; 0", 52),
        ("array_index", "array[STOP] = later(count); 0", 52),
        ("compound_index", "array[STOP] += later(count); 0", 52),
        ("tuple_index", "tuple[STOP] = later(count); 0", 52),
        ("array_receiver", "STOP[later(count)]; 0", 52),
        ("field_receiver", "STOP.value; 0", 52),
        (
            "nested_target",
            "matrix(index(STOP))[grow(1)][0] += grow(2); 0",
            52,
        ),
        (
            "reflect_field_value",
            "set_field(count, \"value\", STOP); 0",
            52,
        ),
        ("reflect_index_value", "set_index(array, 0, STOP); 0", 52),
        (
            "reflect_index",
            "set_index(array, STOP, later(count)); 0",
            52,
        ),
        ("reflect_read_receiver", "get_field(STOP, \"value\"); 0", 52),
        (
            "reflect_write_receiver",
            "set_index(STOP, tick(count), tick(count)); 0",
            52,
        ),
    ]);
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
fn main() -> i32 { val values = Vec::from([1]); consume(fail(values), later(values)) }
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
            .register_host_function(HostFunction::new(standard_log(), move |cx, args| {
                let [Value::Str(id)] = args else {
                    panic!("log string")
                };
                captured
                    .lock()
                    .unwrap()
                    .push(cx.runtime().gc().string(*id).unwrap().to_owned());
                Ok(Value::Unit)
            }))
            .unwrap();
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
        assert_eq!(*effects.lock().unwrap(), vec!["before"]);
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
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
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
    val values: Vec<!> = Vec::from([]);
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
