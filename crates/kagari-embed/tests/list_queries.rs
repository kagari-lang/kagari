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
fn list_queries_apply_to_native_and_readonly_receivers() {
    execute(
        r#"
fn has<T: PartialEq>(xs: List<T>, value: T) -> bool { xs.contains(value) }
fn main() -> i32 {
    val xs = [1, 3, 5];
    val view: List<i32> = xs;
    std::debug::assert(xs.first() == Some(1), "first");
    std::debug::assert(view.last() == Some(5), "last");
    std::debug::assert(has(view, 3), "generic contains");
    std::debug::assert(!view.contains(2), "absent");
    std::debug::assert(view.starts_with([1, 3]), "prefix");
    std::debug::assert(view.ends_with([3, 5]), "suffix");
    std::debug::assert(!view.starts_with([1, 3, 5, 7]), "long prefix");
    std::debug::assert(!view.ends_with([1, 5]), "mismatch suffix");
    std::debug::assert(view.starts_with([]), "empty prefix");
    std::debug::assert(view.ends_with([]), "empty suffix");
    std::debug::assert(view.binary_search(3) == Ok(1usize), "found");
    std::debug::assert(xs.binary_search(0) == Err(0usize), "insert start");
    std::debug::assert(xs.binary_search(4) == Err(2usize), "insert middle");
    std::debug::assert(xs.binary_search(6) == Err(3usize), "insert end");
    val empty: List<i32> = [];
    std::debug::assert(empty.first() == None, "empty first");
    std::debug::assert(empty.last() == None, "empty last");
    std::debug::assert(!empty.contains(1), "empty contains");
    std::debug::assert(empty.binary_search(0) == Err(0usize), "empty search");
    xs.push(7);
    std::debug::assert(xs.last() == Some(7), "guards released");
    42
}
"#,
    );
}

#[test]
fn queries_dispatch_user_equality_and_ordering() {
    execute(
        r#"
struct Rank { val value: i32 }
impl PartialEq for Rank { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl Eq for Rank {}
impl PartialOrd for Rank { fn partial_cmp(self, other: Self) -> Option<Ordering> { self.value.partial_cmp(other.value) } }
impl Ord for Rank { fn cmp(self, other: Self) -> Ordering { self.value.cmp(other.value) } }
struct Sequence { val items: ArrayList<Rank> }
impl Iterable for Sequence { type Item=Rank; type Iter=Iter<Rank>; fn iter(self)->Iter<Rank> {self.items.iter()} }
impl Index<usize> for Sequence {type Output=Rank; fn index(self,index:usize)->Rank {self.items[index]} }
impl List<Rank> for Sequence {
 fn len(self)->usize {self.items.len()}
 fn is_empty(self)->bool {self.items.is_empty()}
 fn get(self,index:usize)->Option<Rank> {self.items.get(index)}
}
fn main() -> i32 {
    val items = [Rank {value:1}, Rank {value:3}, Rank {value:5}];
    val list: List<Rank> = Sequence {items};
    std::debug::assert(list.contains(Rank {value:3}), "custom equality");
    std::debug::assert(list.starts_with([Rank {value:1}]), "custom prefix");
    std::debug::assert(list.binary_search(Rank {value:3}) == Ok(1usize), "custom ordering");
    std::debug::assert(list.binary_search(Rank {value:4}) == Err(2usize), "insertion position");
    42
}
"#,
    );
}

#[test]
fn binary_search_requires_total_order() {
    assert!(
        KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new("no-ord.kgr", "fn main() { [1.0].binary_search(1.0); }"),
                Default::default(),
                Default::default()
            )
            .is_err()
    );
}
