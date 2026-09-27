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
fn lazy_callbacks_and_nested_enum_combinators() {
    execute(
        r#"
struct Counter { var n: i32 }
fn main() -> i32 {
    val count = Counter { n: 0 };
    val none: Option<i32> = None;
    std::debug::assert(Some(7).unwrap_or_else(|| { count.n += 1; 8 }) == 7, "present lazy");
    std::debug::assert(none.unwrap_or_else(|| { count.n += 1; 8 }) == 8, "absent fallback");
    std::debug::assert(count.n == 1, "fallback once");
    std::debug::assert(none.or_else(|| Some(3)) == Some(3), "option recovery");
    std::debug::assert(Some(7).map_or(1, |x| x + 1) == 8, "map");
    std::debug::assert(none.map_or_else(|| 4, |x| x + 1) == 4, "lazy default");
    std::debug::assert(Some(2).filter(|x| x > 3) == None, "filter reject");
    std::debug::assert(Some(7).filter(|x| x > 3) == Some(7), "filter keep");
    std::debug::assert(!none.is_some_and(|x| { count.n += 1; true }), "absent predicate");
    std::debug::assert(count.n == 1, "predicate not called");
    std::debug::assert(Some(1).zip(Some("x")) == Some((1, "x")), "zip");
    std::debug::assert(none.zip(Some("x")) == None, "zip absent left");
    std::debug::assert(Some(7).zip(none) == None, "zip absent right");
    std::debug::assert(Some(Some(7)).flatten() == Some(7), "option flatten");
    val nested_none: Option<Option<i32>> = None;
    std::debug::assert(nested_none.flatten() == None, "outer none");
    val error: Result<i32, String> = Err("missing");
    val good: Result<i32, String> = Ok(7);
    std::debug::assert(error.unwrap_or_else(|e| 9) == 9, "error fallback");
    std::debug::assert(good.unwrap_or_else(|e| { count.n += 1; 0 }) == 7, "ok lazy");
    val recovered: Result<i32, i32> = error.or_else(|e| Ok(4));
    std::debug::assert(recovered == Ok(4), "recovery changes error type");
    std::debug::assert(good.map_or(0, |x| x + 1) == 8, "result map");
    std::debug::assert(error.map_or_else(|e| 6, |x| x + 1) == 6, "error map");
    std::debug::assert(good.ok() == Some(7), "ok projection");
    std::debug::assert(error.ok() == None, "discard error");
    std::debug::assert(error.err() == Some("missing"), "err projection");
    std::debug::assert(good.err() == None, "discard success");
    std::debug::assert(good.is_ok_and(|x| x == 7), "ok predicate");
    std::debug::assert(error.is_err_and(|e| e == "missing"), "err predicate");
    std::debug::assert(!good.is_err_and(|e| { count.n += 1; true }), "skip err predicate");
    std::debug::assert(count.n == 1, "selected callbacks only");
    val rr: Result<Result<i32, String>, String> = Ok(good);
    std::debug::assert(rr.flatten() == good, "result flatten");
    val oe: Option<Result<i32, String>> = Some(error);
    std::debug::assert(oe.transpose() == Err("missing"), "option transpose error");
    val on: Option<Result<i32, String>> = None;
    std::debug::assert(on.transpose() == Ok(None), "option transpose absence");
    val ro: Result<Option<i32>, String> = Ok(Some(7));
    std::debug::assert(ro.transpose() == Some(Ok(7)), "result transpose success");
    val rn: Result<Option<i32>, String> = Ok(None);
    std::debug::assert(rn.transpose() == None, "result transpose absence");
    val re: Result<Option<i32>, String> = Err("missing");
    std::debug::assert(re.transpose() == Some(Err("missing")), "result transpose error");
    42
}
"#,
    );
}

#[test]
fn callback_traps_release_roots_and_allow_next_call() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "callback-cleanup.kgr",
                r#"
fn main() -> i32 {
    val absent: Option<i32> = None;
    absent.unwrap_or_else(|| 1 / 0)
}
fn healthy() -> i32 { 42 }
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
fn generic_combinators_accept_scalar_and_object_results() {
    execute(
        r#"
struct Box { var value: i32 }
fn fallback<T>(value: Option<T>, other: fn() -> T) -> T { value.unwrap_or_else(other) }
fn collapse<T>(value: Option<Option<T>>) -> Option<T> { value.flatten() }
fn main() -> i32 {
    val original = Box { value: 7 };
    val copy = fallback(Some(original), || Box { value: 9 });
    copy.value = 42;
    std::debug::assert(original.value == 42, "shared result");
    std::debug::assert(collapse(Some(Some(42))) == Some(42), "generic nesting");
    42
}
"#,
    );
}
