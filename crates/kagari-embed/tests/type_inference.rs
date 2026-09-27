use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("type-inference.kgr", source),
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
            val keys = MutableSet::new();
            keys.insert(values[0]);
            val map = MutableMap::new();
            map.insert("answer", values[1]);
            std::debug::assert(keys.contains(20), "inferred set");
            std::debug::assert_eq(map.get("answer"), Some(22), "inferred map");
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
        fn record(events: MutableArray<i32>, value: i32) -> i32 {
            events.push(value);
            value
        }
        fn main() -> i32 {
            val events = [];
            val x = consume(Marker { value: record(events, 20) }, record(events, 22));
            std::debug::assert_eq(events[0], 20, "first argument");
            std::debug::assert_eq(events[1], 22, "second argument");
            std::debug::assert_eq(events.len(), "ab".len_bytes(), "exactly once");
            val pair = Pair { marker: Marker { value: x }, seed: true };
            val bundle = Bundle::Pair(Marker { value: x }, true);
            val options = [None, Some(22)];
            val branch = if false { None } else { Some(22) };
            std::debug::assert_eq(options[1], branch, "branch and element context");
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
        fn main() -> i32 {
            val source = [Ok(Some(20)), Ok(Some(22))];
            val checked: Result<Option<Array<i32>>, String> = source.iter().collect();
            val selected: Result<Array<i32>, String> = [20, 22].iter().map(|x| Ok(x)).collect();
            val nested: Array<i32> = [[], [20, 22]].iter().flatten().collect();
            std::debug::assert_eq(selected.is_ok(), true, "callback result inference");
            std::debug::assert_eq(checked.is_ok(), true, "nested collection inference");
            nested.iter().sum()
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
            std::debug::assert_eq(small, 0xff_u8, "suffix and radix");
            std::debug::assert_eq(MINIMUM, -128i8, "negative minimum");
            std::debug::assert_eq(WIDE, 4000000002i64, "wide const");
            std::debug::assert_eq(DOUBLE, 2f64, "double const");
            std::debug::assert(maximum > 9223372036854775808u64, "unsigned comparison");
            std::debug::assert_eq(maximum / 3u64, 6148914691236517205u64, "unsigned division");
            std::debug::assert_eq(f"{size}", "18446744073709551615", "unsigned formatting");
            val keys = MutableSet::new();
            keys.insert(maximum);
            std::debug::assert(keys.contains(maximum), "unsigned hash and equality");
            val single: f32 = 1.25;
            std::debug::assert_eq(single, 1.25f32, "single context");
            val inferred = 2.0;
            std::debug::assert_eq(inferred, 2.0f64, "double fallback");
            std::debug::assert_eq([1, 2].len(), 2usize, "size context");
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
                .compile_source(
                    SourceFile::new(
                        "invalid-number.kgr",
                        format!("fn main() {{ val x = {expression}; }}")
                    ),
                    Default::default()
                )
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
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
        assert!(
            format!("{error:?}").contains("overflow"),
            "{expression}: {error:?}"
        );
    }
}
