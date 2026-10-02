use kagari_common::source::SourceFile;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};

use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("standard-traits.kgr", source),
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
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
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
    }
}

#[test]
fn closures_and_objects_share_callable_bounds() {
    execute(
        r#"
struct Adder { var base: i32 }
impl Fn<(i32,)> for Adder {
    type Output = i32;
    fn call(self, args: (i32,)) -> i32 { self.base += args[0]; self.base }
}
fn apply<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 { f(x) }
fn infer<T, R, F: Fn(T) -> R>(x: T, f: F) -> R { f(x) }
fn two<F: Fn(i32, i32) -> i32>(f: F) -> i32 { f(20, 22) }
fn zero<F: Fn() -> i32>(f: F) -> i32 { f() }
fn unit<F: Fn()>(f: F) { f(); }
fn main() -> i32 {
    val add = Adder { base: 10 };
    { val passed = add(2) == 12; if !passed { return 0; } };
    { val passed = apply(add, 3) == 15; if !passed { return 0; } };
    { val passed = apply(|x| x + 1, 41) == 42; if !passed { return 0; } };
    { val passed = infer(21, |x| x * 2) == 42; if !passed { return 0; } };
    { val passed = two(|x, y| x + y) == 42; if !passed { return 0; } };
    { val passed = zero(|| 42) == 42; if !passed { return 0; } };
    unit(|| { add.base = 0; });
    val f: fn(i32) -> i32 = |x| x + 1;
    { val passed = f.call((41,)) == 42; if !passed { return 0; } };
    add(42)
}
"#,
    );
}

#[test]
fn callable_adapters_keep_shared_receivers_alive_and_work_in_collections() {
    execute(
        r#"
use std::collections;
struct Counter { var value: i32 }
impl Fn<(i32,)> for Counter {
    type Output = i32;
    fn call(self, args: (i32,)) -> i32 { self.value += args[0]; self.value }
}
fn make() -> fn(i32) -> i32 { Counter { value: 0 } }
fn erase<F: Fn(i32) -> i32>(f: F) -> fn(i32) -> i32 { f }
fn consume(f: fn(i32) -> i32) -> i32 { f(20) }
fn main() -> i32 {
    val counter = Counter { value: 0 };
    val f: fn(i32) -> i32 = counter;
    { val passed = f(1) == 1; if !passed { return 0; } };
    { val passed = consume(counter) == 21; if !passed { return 0; } };
    { val passed = erase(counter)(1) == 22; if !passed { return 0; } };
    val mapped = collections::map([1, 2], counter);
    val first = mapped.next(); val second = mapped.next();
    { val passed = first == Some(23) && second == Some(25); if !passed { return 0; } };
    val escaped = make();
    val garbage = [Counter { value: 10 }, Counter { value: 20 }];
    { val passed = garbage.len() == 2; if !passed { return 0; } };
    { val passed = escaped(1) == 1 && escaped(1) == 2; if !passed { return 0; } };
    counter(17)
}
"#,
    );
}

#[test]
fn callable_errors_are_reported_before_codegen() {
    for source in [
        "fn apply<F: Fn(i32) -> i32>(f:F) {} fn main(){apply(|x: String| 1);}",
        "fn apply<F: Fn(i32) -> i32>(f:F) {} fn main(){apply(|x: i32| true);}",
        "struct X {} fn main(){val f:fn(i32)->i32=X{};}",
        "trait Other<T>{type Output;} fn bad<F: Other(i32)->i32>(f:F){}",
        "fn bad<F: Fn(i32)->i32>(f:F){ f(); }",
        "fn bad<F: Fn(i32)->i32>(f:F){ f(1,2); }",
        "fn bad<F: Fn(i32)->i32>(f:F){ f(true); }",
    ] {
        let engine = KagariEngine::default();
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-callable.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn callable_bounds_work_in_methods_and_where_clauses() {
    execute(
        r#"
use core::language::Fn as Callable;
struct Run {}
impl Run {
    fn apply<T, R, F: Callable(T) -> R>(self, x: T, f: F) -> R { f(x) }
}
trait Invoke {
    fn invoke<T, R, F: Fn(T) -> R>(self, x: T, f: F) -> R { f(x) }
}
struct Runner {}
impl Invoke for Runner {}
fn apply<F>(f: F) -> i32 where F: Callable(i32,) -> i32 { f(21) }
fn main() -> i32 {
    { val passed = Run {}.apply(21, |x| x * 2) == 42; if !passed { return 0; } };
    { val passed = Runner {}.invoke(21, |x| x * 2) == 42; if !passed { return 0; } };
    apply(|x| x * 2)
}
"#,
    );
}

#[test]
fn callable_inference_preserves_output_and_evaluation_order() {
    execute(
        r#"
struct Trace { var digits: i32 }
struct Add { val trace: Trace }
impl Fn<(i32, i32)> for Add {
    type Output = i32;
    fn call(self, args: (i32, i32)) -> i32 {
        { val passed = self.trace.digits == 123; if !passed { return 0; } };
        args[0] + args[1]
    }
}
fn receiver(trace: Trace) -> Add { trace.digits = trace.digits * 10 + 1; Add { trace } }
fn argument(trace: Trace, digit: i32, value: i32) -> i32 { trace.digits = trace.digits * 10 + digit; value }
fn first<T, R, F: Fn(T) -> R>(f: F, x: T) -> R { f(x) }
fn projected<F: Fn<(i32,)>>(f: F) -> F::Output { f(21) }
fn twice(x: i32) -> i32 { x * 2 }
fn main() -> i32 {
    { val passed = first(|x| x.len(), [1, 2, 3]) == 3; if !passed { return 0; } };
    { val passed = projected(|x| twice(x)) == 42; if !passed { return 0; } };
    val trace = Trace { digits: 0 };
    receiver(trace)(argument(trace, 2, 20), argument(trace, 3, 22))
}
"#,
    );
}

#[test]
fn callable_traps_release_roots_and_allow_subsequent_execution() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "callable-trap.kgr",
                r#"
use std::collections;
struct Failure {}
impl Fn<(i32,)> for Failure { type Output=i32;
 fn call(self, args:(i32,))->i32 { val zero = 0; args[0] / zero }
}
fn main(){ val mapped = collections::map([1], Failure{}); mapped.next(); }
fn healthy()->i32{42}
"#,
            ),
            Default::default(),
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
    assert!(runtime.execute(&loaded, "main", &[], &context).is_err());
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime
            .execute(&loaded, "healthy", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn native_callbacks_consume_the_coerced_function_signature() {
    execute(
        r#"
use std::collections;
struct Compare {}
impl Fn<(i32, i32)> for Compare {
    type Output = Ordering;
    fn call(self, args: (i32, i32)) -> Ordering { args[0].cmp(args[1]) }
}
struct Increment {}
impl Fn<(i32,)> for Increment {
    type Output = i32;
    fn call(self, args: (i32,)) -> i32 { args[0] + 1 }
}
fn main() -> i32 {
    val xs = [3, 1, 2];
    collections::sort_by(xs, Compare {});
    if xs[0] != 1 || xs[2] != 3 { return 0; }
    val mapped = collections::map(xs, Increment {});
    if mapped.next() != Some(2) { return 0; }
    42
}
"#,
    );
}

#[test]
fn terminating_arguments_do_not_require_a_callable_value() {
    execute(
        r#"
fn invoke<F:Fn(i32)->i32>(f:F)->i32{f(0)}
fn main()->i32{invoke({return 42;})}
"#,
    );
}
