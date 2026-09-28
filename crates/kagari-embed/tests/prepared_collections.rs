use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine, program::PreparedProgram};
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
fn stable_sort_retain_and_adjacent_dedup_commit_prepared_results() {
    execute(
        r#"
struct Item { val key: i32, val tag: i32 }
struct Count { var calls: i32 }
struct Key { val id: i32 }
impl PartialEq for Key { fn eq(self, other: Self) -> bool { self.id == other.id } }
impl Eq for Key {}
impl Hash for Key { fn hash(self) -> i64 { 0i64 } }
fn main() -> i32 {
    val a = [3, 1, 2, 1];
    val view: List<i32> = a;
    a.sort();
    std::debug::assert(view[0usize] == 1 && view[3usize] == 3, "live sorted view");
    a.sort_by(|a, b| b.cmp(a));
    std::debug::assert(a[0usize] == 3 && a[3usize] == 1, "descending");
    a.dedup();
    std::debug::assert(a.len() == 3usize, "dedup");
    a.retain(|n| n > 1);
    std::debug::assert(a.len() == 2usize && a[1usize] == 2, "retain order");
    val duplicate = [1, 1, 2, 1]; duplicate.dedup();
    std::debug::assert(duplicate.len() == 3usize, "adjacent only");
    val calls = Count { calls: 0 };
    val items = [Item { key: 2, tag: 0 }, Item { key: 1, tag: 1 }, Item { key: 2, tag: 2 }];
    items.sort_by_key(|v| { calls.calls += 1; v.key });
    std::debug::assert(calls.calls == 3, "key once");
    std::debug::assert(items[0usize].tag == 1 && items[1usize].tag == 0 && items[2usize].tag == 2, "stable");
    items.sort_by(|a, b| a.key.cmp(b.key));
    std::debug::assert(items[1usize].tag == 0 && items[2usize].tag == 2, "stable comparator");
    val empty: ArrayList<i32> = []; empty.sort(); empty.sort_by_key(|n| n); empty.dedup(); empty.retain(|n| true);
    for size in 0usize..20usize {
        val data = ArrayList::from_fn(size, |i| (size - i) as i32);
        data.sort();
        for i in 0usize..size { std::debug::assert(data[i] == (i + 1usize) as i32, "merge runs"); }
    }
    val map = LinkedHashMap::from([(Key { id: 1 }, 10), (Key { id: 2 }, 20)]);
    map.retain(|k, v| k.id == 2 && v == 20);
    std::debug::assert(map.get(Key { id: 2 }) == Some(20) && map.len() == 1usize, "custom retained keys");
    val set = LinkedHashSet::from([Key { id: 1 }, Key { id: 2 }]);
    set.retain(|k| k.id == 2);
    std::debug::assert(set.contains(Key { id: 2 }) && set.len() == 1usize, "set retain");
    42
}
"#,
    );
}
