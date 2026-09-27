use kagari_common::SourceFile;
use kagari_embed::BytecodeArtifact;
use kagari_embed::ExecutionContext;
use kagari_embed::KagariEngine;
use kagari_embed::program::PreparedProgram;
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
fn windows_and_chunks_are_lazy_independent_shallow_snapshots() {
    execute(
        r#"
struct Item { var value: i32 }
struct Proxy { val values: ArrayList<i32>, val reads: ArrayList<i32> }
impl Index<usize> for Proxy { type Output = i32; fn index(self, i: usize) -> i32 { self.values[i] } }
impl Iterable for Proxy { type Item = i32; type Iter = Iter<i32>; fn iter(self) -> Iter<i32> { self.values.iter() } }
impl List<i32> for Proxy {
    fn len(self) -> usize { self.values.len() }
    fn is_empty(self) -> bool { self.values.is_empty() }
    fn get(self, i: usize) -> Option<i32> { self.reads.push(i as i32); self.values.get(i) }
}
fn main() -> i32 {
    val values = [1, 2, 3];
    val view: List<i32> = values;
    val windows = view.windows(2usize);
    values[0usize] = 9;
    val first = windows.next().unwrap_or([]);
    std::debug::assert(first[0usize] == 9 && first[1usize] == 2, "snapshot at next");
    values[1usize] = 8;
    val second = windows.next().unwrap_or([]);
    std::debug::assert(first[1usize] == 2 && second[0usize] == 8, "independent slots");
    std::debug::assert(windows.next().is_none() && windows.next().is_none(), "fused end");
    values.push(4);
    val chunks: ArrayList<List<i32>> = values.chunks(3usize).collect();
    std::debug::assert(chunks.len() == 2usize && chunks[1usize].len() == 1usize, "short tail");
    val empty: List<i32> = [];
    std::debug::assert(empty.windows(1usize).next().is_none(), "empty");
    std::debug::assert(values.windows(9usize).next().is_none(), "oversized");
    val objects = [Item { value: 1 }];
    val piece = objects.chunks(1usize).next().unwrap_or([]);
    piece[0usize].value = 42;
    std::debug::assert(objects[0usize].value == 42, "shared object");
    val reads: ArrayList<i32> = [];
    val custom: List<i32> = Proxy { values: [1, 2, 3], reads };
    val lazy = custom.windows(2usize);
    std::debug::assert(reads.is_empty(), "no eager reads");
    val snapshots: ArrayList<List<i32>> = lazy.collect();
    std::debug::assert(reads.len() == 4usize && snapshots.len() == 2usize, "custom list");
    val resumed = values.windows(2usize);
    val head: ArrayList<List<i32>> = resumed.take(1usize).collect();
    std::debug::assert(resumed.next().unwrap_or([])[0usize] == 8, "reopen unchanged source");
    val tail: ArrayList<List<i32>> = resumed.collect();
    std::debug::assert(head.len() == 1usize && tail.len() == 1usize, "remaining windows");
    val prefix: ArrayList<List<i32>> = values.windows(2usize).take(1usize).collect();
    values.push(5);
    std::debug::assert(prefix.len() == 1usize, "early close");
    42
}
"#,
    );
}

#[test]
fn range_removal_returns_readonly_storage_after_immediate_commit() {
    execute(
        r#"
fn main() -> i32 {
    val a = [1, 2, 3, 4, 5];
    val alias: List<i32> = a;
    val removed = a.remove_range(1usize..=3usize);
    std::debug::assert(removed.len() == 3usize && removed[0usize] == 2, "removed");
    std::debug::assert(alias.len() == 2usize && alias[1usize] == 5, "immediate commit");
    a[0usize] = 9;
    std::debug::assert(removed[0usize] == 2, "separate slots");
    std::debug::assert(a.remove_range(2usize..2usize).is_empty(), "empty range");
    std::debug::assert(a.remove_range(..1usize).len() == 1usize, "prefix");
    std::debug::assert(a.remove_range(..).len() == 1usize && a.is_empty(), "full range");
    42
}
"#,
    );
}
