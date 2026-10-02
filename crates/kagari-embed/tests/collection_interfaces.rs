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
fn native_collection_views() {
    execute(
        r#"
fn size(xs: List<i32>) -> usize { xs.len() }
fn change(xs: MutableList<i32>) { xs.push(42); xs[0] += 6; }
fn main() -> i32 {
    val xs = [1];
    change(xs);
    val view: MutableList<i32> = xs;
    val read: [i32] = view;
    if !(read[0usize] == 7) { return 0; }
    var total = 0;
    for item in read { total += item; }
    if !(total == 49) { return 0; }
    val map: HashMap<String, i32> = HashMap::new();
    val writable: MutableMap<String, i32> = map;
    writable.insert("answer", 42);
    val readable: Map<String, i32> = writable;
    if !(readable.get("answer") == Some(42)) { return 0; }
    val set: HashSet<i32> = HashSet::new();
    val writable_set: MutableSet<i32> = set;
    writable_set.insert(42);
    val readable_set: Set<i32> = writable_set;
    if !(readable_set.contains(42)) { return 0; }
    if !(size(xs) == 2usize) { return 0; }
    if !(read.get(0) == Some(7)) { return 0; }
    42
}
"#,
    );
}

#[test]
fn user_containers_implement_the_same_storage_independent_contracts() {
    execute(
        r#"
struct Sequence { val items: ArrayList<i32> }
impl Index<usize> for Sequence {
    type Output = i32;
    fn index(self, index: usize) -> i32 { self.items[index] }
}
impl Iterable for Sequence {
    type Item = i32;
    type Iter = CollectionCursor<i32>;
    fn iter(self) -> CollectionCursor<i32> { self.items.iter() }
}
impl List<i32> for Sequence {
    fn len(self) -> usize { self.items.len() }
    fn is_empty(self) -> bool { self.items.is_empty() }
    fn get(self, index: usize) -> Option<i32> { self.items.get(index) }
}
impl MutableList<i32> for Sequence {

    fn push(self, value: i32) { self.items.push(value); }
    fn pop(self) -> Option<i32> { self.items.pop() }
    fn insert(self, index: usize, value: i32) { self.items.insert(index, value); }
    fn remove(self, index: usize) -> Option<i32> { self.items.remove(index) }
    fn clear(self) { self.items.clear(); }
    fn set(self, index: usize, value: i32) { self.items[index] = value; }
}
struct Singleton { val value: f64 }
impl Iterable for Singleton {
    type Item = f64;
    type Iter = CollectionCursor<f64>;
    fn iter(self) -> CollectionCursor<f64> { [self.value].iter() }
}
impl Set<f64> for Singleton {
    fn len(self) -> usize { 1usize }
    fn is_empty(self) -> bool { false }
    fn contains(self, value: f64) -> bool { self.value == value }
}
fn first<T>(xs: [T]) -> T { xs[0] }
fn size<C: List<i32>>(xs: C) -> usize { xs.len() }
fn main() -> i32 {
    val source = Sequence { items: [7, 35] };
    val view: [i32] = source;
    val copy = [view[0], view[1]];
    val copied_view: List<i32> = copy;
    if !(!(view === copied_view) && view != copied_view) { return 0; }
    val writable: MutableList<i32> = source;
    writable[0] -= 6;
    if !(copy[0] == 7 && view[0] == 1) { return 0; }
    if !(size(source) == 2usize && size(copy) == 2usize) { return 0; }
    val set: Set<f64> = Singleton { value: 0.5 };
    if !(set.contains(0.5)) { return 0; }
    first(copy) + copy[1]
}
"#,
    );
}

#[test]
fn hash_collection_interfaces_use_custom_key_protocols() {
    execute(
        r#"
struct Key { val value: i32 }
impl PartialEq for Key { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl Eq for Key {}
impl Hash for Key { fn hash(self) -> i64 { self.value.hash() } }
fn main() -> i32 {
    val storage: HashMap<Key, i32> = HashMap::new();
    val map: MutableMap<Key, i32> = storage;
    map.insert(Key { value: 1 }, 40);
    map.insert(Key { value: 1 }, 42);
    for (key, value) in storage { if key.value != 1 || value != 42 { return 0; } }
    val read: Map<Key, i32> = map;
    val lookup = Key { value: 1 };
    if !(read.len() == 1usize && read.get(lookup) == Some(42)) { return 0; }
    val set: MutableSet<Key> = HashSet::new();
    set.insert(Key { value: 1 });
    set.insert(Key { value: 1 });
    if !(set.len() == 1usize && set.contains(lookup)) { return 0; }
    42
}
"#,
    );
}

#[test]
fn readonly_views_do_not_grant_mutators_or_implicit_storage_construction() {
    let engine = KagariEngine::default();
    for source in [
        "fn main() { val xs: [i32] = [1]; xs[0] = 2; }",
        "fn main() { val xs: MutableList<i32> = [1]; xs[true] = 2; }",
        "fn main() { val xs: MutableList<i32> = [1]; xs[0i32] = 2; }",
        "fn main() { val xs: List<i32> = [1]; val ys: MutableList<i32> = xs; }",
        "fn main() { val xs: Map<i32,i32> = Map::new(); }",
        "fn main() { val xs: Set<f64> = HashSet::new(); }",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("rejected.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn view_identity_survives_upcasts_branches_and_hash_storage() {
    execute(
        r#"
struct Cell { var value: i32 }
fn readonly(xs: MutableList<Cell>) -> List<Cell> { xs }
fn main() -> i32 {
    val raw = [Cell { value: 20 }];
    val writable: MutableList<Cell> = raw;
    val read: List<Cell> = readonly(writable);
    read[0].value += 22;
    val joined = if true { raw } else { read };
    val selected = match 1 { 1 => raw, _ => read };
    if !(raw === read && writable == read && selected === joined) { return 0; }
    if !(raw.hash() == read.hash()) { return 0; }
    val views: HashSet<List<Cell>> = HashSet::new();
    views.insert(read); views.insert(joined); views.insert(selected);
    if !(views.len() == 1usize) { return 0; }
    raw[0].value
}
"#,
    );
}
