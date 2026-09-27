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
