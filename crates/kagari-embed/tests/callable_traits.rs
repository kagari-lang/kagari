use crate::BytecodeArtifact;
use crate::ExecutionContext;
use crate::KagariEngine;
use kagari_common::SourceFile;
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
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
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
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
    std::debug::assert(add(2) == 12, "object call");
    std::debug::assert(apply(add, 3) == 15, "generic object");
    std::debug::assert(apply(|x| x + 1, 41) == 42, "closure context");
    std::debug::assert(infer(21, |x| x * 2) == 42, "inferred output");
    std::debug::assert(two(|x, y| x + y) == 42, "two args");
    std::debug::assert(zero(|| 42) == 42, "zero args");
    unit(|| { add.base = 0; });
    val f: fn(i32) -> i32 = |x| x + 1;
    std::debug::assert(f.call((41,)) == 42, "explicit tuple call");
    add(42)
}
"#,
    );
}

#[test]
fn callable_adapters_keep_shared_receivers_alive_and_work_in_collections() {
    execute(
        r#"
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
    std::debug::assert(f(1) == 1, "assigned adapter");
    std::debug::assert(consume(counter) == 21, "argument adapter");
    std::debug::assert(erase(counter)(1) == 22, "generic adapter");
    val xs: ArrayList<i32> = [1, 2].iter().map(counter).collect();
    std::debug::assert(xs[0] == 23 && xs[1] == 25, "iterator callback");
    val escaped = make();
    val garbage = [Counter { value: 10 }, Counter { value: 20 }];
    std::debug::assert(garbage.len() == 2, "allocate between calls");
    std::debug::assert(escaped(1) == 1 && escaped(1) == 2, "capture root");
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
use std::ops::Fn as Callable;
struct Run {}
impl Run {
    fn apply<T, R, F: Callable(T) -> R>(self, x: T, f: F) -> R { f(x) }
}
trait Invoke {
    fn invoke<T, R, F: Fn(T) -> R>(self, x: T, f: F) -> R { f(x) }
}
struct Runner {}
impl Invoke for Runner {}
fn apply<F>(f: F) -> i32 where F: std::ops::Fn(i32,) -> i32 { f(21) }
fn main() -> i32 {
    std::debug::assert(Run {}.apply(21, |x| x * 2) == 42, "inherent method");
    std::debug::assert(Runner {}.invoke(21, |x| x * 2) == 42, "trait method");
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
        std::debug::assert(self.trace.digits == 123, "receiver and arguments evaluated once");
        args[0] + args[1]
    }
}
fn receiver(trace: Trace) -> Add { trace.digits = trace.digits * 10 + 1; Add { trace } }
fn argument(trace: Trace, digit: i32, value: i32) -> i32 { trace.digits = trace.digits * 10 + digit; value }
fn first<T, R, F: Fn(T) -> R>(f: F, x: T) -> R { f(x) }
fn projected<F: Fn<(i32,)>>(f: F) -> F::Output { f(21) }
fn twice(x: i32) -> i32 { x * 2 }
fn main() -> i32 {
    std::debug::assert(first(|x| x.len_chars(), "abc") == 3, "later argument context");
    std::debug::assert(projected(|x| twice(x)) == 42, "function value and associated output");
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
struct Failure {}
impl Fn<(i32,)> for Failure { type Output=i32;
 fn call(self, args:(i32,))->i32 { std::debug::assert(false,"callback failed"); args[0] }
}
fn main(){ val xs:ArrayList<i32> = [1].iter().map(Failure{}).collect(); }
fn healthy()->i32{42}
"#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
fn standard_callbacks_consume_the_coerced_function_signature() {
    execute(
        r#"
struct Identity {}
impl Fn<(i32,)> for Identity { type Output=i32; fn call(self,args:(i32,))->i32 {args[0]} }
struct Factory {}
impl Fn<()> for Factory { type Output=i32; fn call(self,args:())->i32 {42} }
struct Update {}
impl Fn<(Option<i32>,)> for Update {type Output=i32; fn call(self,args:(Option<i32>,))->i32 {args[0].unwrap_or(40)+2} }
struct Keep {}
impl Fn<(i32,)> for Keep {type Output=bool; fn call(self,args:(i32,))->bool {args[0]>1} }
fn main()->i32 {
    val xs=[3,1,2];
    xs.sort_by_key(Identity{});
    std::debug::assert(xs[0]==1 && xs[2]==3,"sort callback output");
    xs.retain(Keep{});
    std::debug::assert(xs.len()==2,"retain callback");
    val m=LinkedHashMap::new();
    std::debug::assert(m.get_or_insert_with("a",Factory{})==42,"zero argument callback");
    std::debug::assert(m.update("b",Update{})==42,"nested argument callback");
    val absent: Option<i32> = None;
    std::debug::assert(absent.unwrap_or_else(Factory{})==42,"lazy optional callback");
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
