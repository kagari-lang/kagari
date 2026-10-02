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
            SourceFile::new("type-inference.kgr", source),
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
    }
}

#[test]
fn later_collection_uses_preserve_access_and_runtime_values() {
    execute(
        r#"
        fn main() -> i32 {
            val values = [];
            val alias = values;
            alias.push(20);
            values.push(22);
            val keys = HashSet::new();
            keys.insert(values[0]);
            val map = HashMap::new();
            map.insert("answer", values[1]);
            { val passed = keys.contains(20); if !passed {val zero=0;1/zero;} };
            { val passed = map.get("answer") == Some(22); if !passed {val zero=0;1/zero;} };
            values[0] + values[1]
        }
    "#,
    );
}

#[test]
fn inference_order_does_not_change_evaluation_order() {
    execute(
        r#"
        struct Marker<T> { val value: i32 }
        struct Pair<T> { val marker: Marker<T>, val seed: T }
        enum Bundle<T> { Pair(Marker<T>, T) }
        fn consume<T>(marker: Marker<T>, seed: T) -> i32 { marker.value }
        fn record(events: ArrayList<i32>, value: i32) -> i32 {
            events.push(value);
            value
        }
        fn main() -> i32 {
            val events = [];
            val x = consume(Marker { value: record(events, 20) }, record(events, 22));
            { val passed = events[0] == 20; if !passed {val zero=0;1/zero;} };
            { val passed = events[1] == 22; if !passed {val zero=0;1/zero;} };
            { val passed = events.len() == 2usize; if !passed {val zero=0;1/zero;} };
            val pair = Pair { marker: Marker { value: x }, seed: true };
            val bundle = Bundle::Pair(Marker { value: x }, true);
            val options = [None, Some(22)];
            val branch = if false { None } else { Some(22) };
            { val passed = options[1] == branch; if !passed {val zero=0;1/zero;} };
            val callback = |value| value + 1;
            callback(41)
        }
    "#,
    );
}

#[test]
fn expected_collection_types_constrain_sources_and_callbacks() {
    execute(
        r#"
        use std::collections;
        fn main() -> i32 {
            val source = [20, 22];
            val selected: collections::MapIterator<i32,Result<i32,String>> = collections::map(source, |x| Ok(x));
            var total=0;
            for item in selected {match item {Ok(n)=>{total+=n;},Err(_)=>{return 0;}};}
            val nested: ArrayList<ArrayList<i32>> = [[], [total]];
            nested[1][0]
        }
    "#,
    );
}

#[test]
fn numeric_suffixes_context_and_full_unsigned_range_execute() {
    execute(
        r#"
        const MINIMUM: i8 = -128i8;
        const WIDE: i64 = 4_000_000_000i64 + 2i64;
        const DOUBLE: f64 = 1.25 + 0.75;
        fn main() -> i32 {
            val small: u8 = 255;
            val signed: i16 = -32768;
            val wide = 9223372036854775807i64;
            val maximum = 18446744073709551615u64;
            val size = 18446744073709551615usize;
            { val passed = small == 0xff_u8; if !passed {val zero=0;1/zero;} };
            { val passed = MINIMUM == -128i8; if !passed {val zero=0;1/zero;} };
            { val passed = WIDE == 4000000002i64; if !passed {val zero=0;1/zero;} };
            { val passed = DOUBLE == 2f64; if !passed {val zero=0;1/zero;} };
            { val passed = maximum > 9223372036854775808u64; if !passed {val zero=0;1/zero;} };
            { val passed = maximum / 3u64 == 6148914691236517205u64; if !passed {val zero=0;1/zero;} };
            { val passed = f"{size}" == "18446744073709551615"; if !passed {val zero=0;1/zero;} };
            val keys = HashSet::new();
            keys.insert(maximum);
            { val passed = keys.contains(maximum); if !passed {val zero=0;1/zero;} };
            val single: f32 = 1.25;
            { val passed = single == 1.25f32; if !passed {val zero=0;1/zero;} };
            val inferred = 2.0;
            { val passed = inferred == 2.0f64; if !passed {val zero=0;1/zero;} };
            { val passed = [1,2].len() == 2usize; if !passed {val zero=0;1/zero;} };
            42
        }
    "#,
    );
}

#[test]
fn invalid_numeric_literals_are_rejected_before_execution() {
    for expression in [
        "256u8",
        "128i8",
        "-129i8",
        "-1u8",
        "18446744073709551616u64",
        "1.0u8",
        "1wat",
        "1e999f64",
    ] {
        let engine = KagariEngine::default();
        assert!(
            engine
                .compile_source(SourceFile::new(
                    "invalid-number.kgr",
                    format!("fn main() {{ val x = {expression}; }}")
                ))
                .is_err(),
            "{expression}"
        );
    }
}

#[test]
fn narrow_and_unsigned_arithmetic_trap_on_overflow() {
    for expression in [
        "255u8 + 1u8",
        "127i8 + 1i8",
        "18446744073709551615u64 + 1u64",
        "0usize - 1usize",
    ] {
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "numeric-overflow.kgr",
                    format!("fn main() {{ val x = {expression}; }}"),
                ),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
        assert!(
            format!("{error:?}").contains("overflow"),
            "{expression}: {error:?}"
        );
    }
}

#[test]
fn explicit_type_arguments_and_local_placeholders_execute() {
    execute(
        r#"
        use std::collections;
        fn identity<T>(value: T) -> T { value }
        trait Transform {
            fn transform<T>(self, value: T) -> T;
        }
        struct Worker {}
        impl Transform for Worker {
            fn transform<T>(self, value: T) -> T { value }
        }
        fn main() -> i32 {
            val values: ArrayList<_> = [20, 22];
            val copy = identity::<ArrayList<i32>>(values);
            val mapped = collections::map::<i32,i64>([42], |x| 42i64).next();
            { val passed = mapped == Some(42i64); if !passed {val zero=0;1/zero;} };
            val success = Ok::<i32, String>(42);
            { val passed = match success {Ok(_)=>true,Err(_)=>false}; if !passed {val zero=0;1/zero;} };
            val answer = identity::<_>(copy[0]) + Worker {}.transform::<i32>(copy[1]);
            identity::<i32>(answer)
        }
    "#,
    );
}

#[test]
fn invalid_explicit_arguments_and_unresolved_holes_are_rejected() {
    for source in [
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<i32>(true); }",
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<i32, bool>(1); }",
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<>(1); }",
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<Missing>(1); }",
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<T = i32>(1); }",
        "fn main() { val f = |x: i32| x; f::<i32>(1); }",
        "fn main() { val values: ArrayList<_> = []; }",
        "fn main(x: _) {}",
        "struct Bad { val field: _ } fn main() {}",
        "fn main() { val x: i64 = Some(1).map::<i32>(|x| x).unwrap(); }",
        "fn main() { val x: Result<bool, String> = Ok::<i32, String>(1); }",
    ] {
        assert!(
            KagariEngine::default()
                .compile_source(SourceFile::new("invalid-inference.kgr", source))
                .is_err(),
            "unexpectedly accepted: {source}"
        );
    }
}
