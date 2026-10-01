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
    std::debug::assert(read[0usize] == 7, "index");
    var total = 0;
    for item in read { total += item; }
    std::debug::assert(total == 49, "iteration");
    val map: LinkedHashMap<String, i32> = LinkedHashMap::new();
    val writable: MutableMap<String, i32> = map;
    writable.insert("answer", 42);
    val readable: Map<String, i32> = writable;
    std::debug::assert(readable.get("answer") == Some(42), "map view");
    val set: LinkedHashSet<i32> = LinkedHashSet::new();
    val writable_set: MutableSet<i32> = set;
    writable_set.insert(42);
    val readable_set: Set<i32> = writable_set;
    std::debug::assert(readable_set.contains(42), "set view");
    std::debug::assert(size(xs) == 2usize, "size");
    std::debug::assert(read.get(0) == Some(7), "shared");
    42
}
"#,
    );
}

#[test]
fn native_list_equality_preserves_composition_identity_and_inherited_views() {
    execute(
        r#"
struct Key {val value:ArrayList<i32>}
impl PartialEq for Key {fn eq(self,other:Self)->bool {
val absent:Option<i32> =None;self.value[0]==absent.unwrap_or_else(||other.value[0])}}
enum Choice<T> {Empty,Item(T)}
fn check<L:List<(Option<Key>,Choice<Key>)>>(source:L)->bool {
source.contains((Some(Key{value:[42]}),Choice::Item(Key{value:[7]})))}
fn main()->i32 {
val value=(Some(Key{value:[42]}),Choice::Item(Key{value:[7]}));
val values=[value];val inherited:MutableList<(Option<Key>,Choice<Key>)> =values;
std::debug::assert(check(values),"generic nested equality");
std::debug::assert(inherited.contains((Some(Key{value:[42]}),Choice::Item(Key{value:[7]}))),"inherited contains");
std::debug::assert(inherited.starts_with([value]) && inherited.ends_with([value]),"same composite");
std::debug::assert(values.starts_with(values) && values.ends_with(values),"aliased guards");
inherited.push(value);
val a=[1];val b=[1];val x:List<i32> =a;val y:List<i32> =b;
std::debug::assert([a].contains(a) && ![a].contains(b),"storage identity");
std::debug::assert([x].contains(x) && ![x].contains(y),"collection view identity");
std::debug::assert([x].starts_with([x]) && ![x].ends_with([y]),"view sequence identity");
std::debug::assert([(Some(1),Choice::Item(2))].contains((Some(1),Choice::Item(2))),"primitive composition");
std::debug::assert(["a","b"].ends_with(["b"]),"string equality");
val nan="NaN".parse::<f64>().unwrap_or(0.0);
std::debug::assert(![nan].contains(nan) && ![nan].starts_with([nan]),"IEEE equality");42
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
    type Iter = Iter<i32>;
    fn iter(self) -> Iter<i32> { self.items.iter() }
}
impl List<i32> for Sequence {
    fn len(self) -> usize { self.items.len() }
    fn is_empty(self) -> bool { self.items.is_empty() }
    fn get(self, index: usize) -> Option<i32> { self.items.get(index) }
}
impl MutableList<i32> for Sequence {
    fn swap(self,a:usize,b:usize) {self.items.swap(a,b);}
    fn reverse(self) {self.items.reverse();}
    fn truncate(self,len:usize) {self.items.truncate(len);}
    fn extend(self,source:List<i32>) {self.items.extend(source);}

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
    type Iter = Iter<f64>;
    fn iter(self) -> Iter<f64> { [self.value].iter() }
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
    val copy = ArrayList::from(view);
    val copied_view: List<i32> = copy;
    std::debug::assert(!(view === copied_view) && view != copied_view, "different implementations");
    val writable: MutableList<i32> = source;
    writable[0] -= 6;
    std::debug::assert(copy[0] == 7 && view[0] == 1, "custom list view");
    std::debug::assert(size(source) == 2usize && size(copy) == 2usize, "static bounds");
    val set: Set<f64> = Singleton { value: 0.5 };
    std::debug::assert(set.contains(0.5), "set interface does not require Hash");
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
    val storage: LinkedHashMap<Key, i32> = LinkedHashMap::new();
    val map: MutableMap<Key, i32> = storage;
    map.insert(Key { value: 1 }, 40);
    map.insert(Key { value: 1 }, 42);
    val snapshot = storage.keys();
    std::debug::assert(snapshot[0].value == 1, "custom key snapshot");
    val read: Map<Key, i32> = map;
    std::debug::assert(read.len() == 1usize && read.get(Key { value: 1 }) == Some(42), "custom key");
    val set: MutableSet<Key> = LinkedHashSet::new();
    set.insert(Key { value: 1 });
    set.insert(Key { value: 1 });
    std::debug::assert(set.len() == 1usize && set.contains(Key { value: 1 }), "set key");
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
        "fn main() { val xs: List<i32> = [1].iter().collect(); }",
        "fn main() { val xs: Map<i32,i32> = Map::new(); }",
        "fn main() { val xs: Set<f64> = LinkedHashSet::new(); }",
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
    std::debug::assert(raw === read && writable == read && selected === joined, "one object");
    std::debug::assert(raw.hash() == read.hash(), "one identity hash");
    val views = LinkedHashSet::from([read, joined, selected]);
    std::debug::assert(views.len() == 1usize, "view keys");
    raw[0].value
}
"#,
    );
}

#[test]
fn map_snapshots_are_readonly_ordered_and_shallow() {
    execute(
        r#"
struct Cell { var value: i32 }
fn keys<K: Eq + Hash, V>(map: LinkedHashMap<K,V>) -> List<K> { map.keys() }
fn main() -> i32 {
    val cell = Cell { value: 20 };
    val map = LinkedHashMap::from([("first", cell), ("second", Cell { value: 7 })]);
    val ks = keys(map);
    val vs = map.values();
    val es = std::map::LinkedHashMap::entries(map);
    map.insert("first", Cell { value: 99 });
    map.remove("second");
    map.insert("third", Cell { value: 100 });
    std::debug::assert(ks[0] == "first" && ks[1] == "second", "ordered key snapshot");
    std::debug::assert(vs[0] === cell && vs[1].value == 7, "independent slots");
    vs[0].value += 22;
    std::debug::assert(es[0][1].value == 42, "shallow object references");
    val writable = ArrayList::from(ks);
    writable[0] = "changed";
    writable.push("extra");
    std::debug::assert(ks.len() == 2usize && ks[0] == "first", "explicit writable copy");
    val empty: LinkedHashMap<String,i32> = LinkedHashMap::new();
    std::debug::assert(empty.keys().is_empty() && empty.values().is_empty() && empty.entries().is_empty(), "empty snapshots");
    var total = 0;
    for key in ks { total += key.len_bytes() as i32; }
    std::debug::assert(total == 11, "snapshot iteration");
    cell.value
}
"#,
    );
}

#[test]
fn map_snapshot_return_types_reject_writes_without_annotations() {
    let engine = KagariEngine::default();
    for operation in [
        "map.keys().push(2);",
        "map.values()[0] = 3;",
        "map.entries().clear();",
        "val values: ArrayList<i32> = map.values();",
        "val entries: MutableList<(i32,i32)> = map.entries();",
    ] {
        let source =
            format!("fn main() {{ val map = LinkedHashMap::from([(1, 2)]); {operation} }}");
        assert!(
            engine
                .compile_source(
                    SourceFile::new("snapshot-write.kgr", source),
                    Default::default()
                )
                .is_err(),
            "{operation}"
        );
    }
}

#[test]
fn map_snapshots_reject_calls_that_bypass_native_contracts() {
    use kagari_abi::{
        callable::{EngineNativeBinding, NativeCall},
        standard::RuntimePrimitive,
    };
    use kagari_bytecode::{BytecodeInstruction, CallTarget, verify_program};
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(
        SourceFile::new("snapshot-wire.kgr", "fn main() { val map = LinkedHashMap::from([(1,2)]); map.keys(); map.values(); map.entries(); }"),
        Default::default(), Default::default()).unwrap();
    verify_program(&artifact.program).unwrap();
    for public in [
        RuntimePrimitive::MapKeys,
        RuntimePrimitive::MapValues,
        RuntimePrimitive::MapEntries,
    ] {
        let mut forged = artifact.program.clone();
        let root = &mut forged.modules[artifact.program.root.index()];
        let import = root
            .native_imports
            .iter()
            .position(|import| import.binding == EngineNativeBinding::Intrinsic(public))
            .unwrap();
        let mut replaced = false;
        for function in &mut root.functions {
            for instruction in &mut function.instructions {
                if let BytecodeInstruction::Call { callee, .. } = instruction
                    && matches!(callee,CallTarget::Native(NativeCall::Engine(id)) if id.index()==import)
                {
                    *callee = CallTarget::RuntimePrimitive(public);
                    replaced = true;
                }
            }
        }
        assert!(replaced);
        assert!(verify_program(&forged).is_err(), "{public:?}");
    }
}

#[test]
fn string_join_reads_lists_snapshots_and_iterator_progress() {
    execute(
        r#"
fn join_list<C: List<String>>(source: C) -> String { source.join("/") }
fn join_iter<I: Iterator<Item = String>>(source: I) -> String { source.join("/") }
fn main() -> i32 {
    val storage = ["Alice", "Bob"];
    val view: List<String> = storage;
    val writable: MutableList<String> = storage;
    std::debug::assert(view.join(", ") == "Alice, Bob", "list");
    std::debug::assert(writable.join(", ") == "Alice, Bob", "inherited list");
    std::debug::assert(join_list(storage) == "Alice/Bob", "generic list");
    val empty: List<String> = [];
    std::debug::assert(empty.join(",") == "", "empty");
    val one: List<String> = ["中文😀"];
    std::debug::assert(one.join(",") == "中文😀", "singleton");
    val parts: List<String> = ["", "", ""];
    std::debug::assert(parts.join("😀") == "😀😀", "empty parts");
    val map = LinkedHashMap::from([("first", 1), ("second", 2)]);
    std::debug::assert(map.keys().join("/") == "first/second", "snapshot");
    std::debug::assert([1, 2, 3].iter().map(|x| f"{x}").join(",") == "1,2,3", "format pipeline");
    val cursor = storage.iter();
    cursor.next();
    std::debug::assert(join_iter(cursor) == "Bob", "remaining items");
    std::debug::assert(cursor.next() == None, "exhausted");
    storage.push("Carol");
    42
}
"#,
    );
}

#[test]
fn string_join_rejects_non_string_items_before_codegen() {
    let engine = KagariEngine::default();
    for source in [
        "fn main() { [1, 2].join(\",\"); }",
        "fn main() { val xs: List<i32> = [1]; xs.join(\",\"); }",
        "fn main() { [1].iter().join(\",\"); }",
        "fn bad<T>(xs: List<T>) -> String { xs.join(\",\") }",
        "fn bad<I: Iterator>(xs: I) -> String { xs.join(\",\") }",
        r#"struct Source {} impl Iterator for Source {
            type Item = String;
            fn next(self) -> Option<String> { None }
            fn join(self, separator: String) -> String { "override" }
        } fn main() {}"#,
    ] {
        assert!(
            engine
                .compile_source(
                    SourceFile::new("invalid-join.kgr", source),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn string_join_supports_custom_sources_and_stops_at_first_none() {
    execute(
        r#"
struct Sequence { val items: ArrayList<String> }
impl Index<usize> for Sequence {
    type Output = String;
    fn index(self, index: usize) -> String { self.items[index] }
}
impl Iterable for Sequence {
    type Item = String;
    type Iter = Iter<String>;
    fn iter(self) -> Iter<String> { self.items.iter() }
}
impl List<String> for Sequence {
    fn len(self) -> usize { self.items.len() }
    fn is_empty(self) -> bool { self.items.is_empty() }
    fn get(self, index: usize) -> Option<String> { self.items.get(index) }
}
struct Sometimes { var calls: i32 }
impl Iterator for Sometimes {
    type Item = String;
    fn next(self) -> Option<String> {
        self.calls += 1;
        if self.calls == 2 { None } else { Some(f"{self.calls}") }
    }
}
fn main() -> i32 {
    val custom = Sequence { items: ["a", "b"] };
    std::debug::assert(custom.join("-") == "a-b", "concrete custom list");
    val view: List<String> = custom;
    std::debug::assert(view.join("-") == "a-b", "dynamic custom list");
    val source = Sometimes { calls: 0 };
    std::debug::assert(source.join(",") == "1" && source.calls == 2, "first None");
    std::debug::assert(source.next() == Some("3"), "no extra next");
    val calls = Sometimes { calls: 0 };
    val text = [1, 2, 3].iter().map(|x| { calls.calls += 1; f"{x}" }).join("/");
    std::debug::assert(text == "1/2/3" && calls.calls == 3, "exactly once");
    42
}
"#,
    );
}

#[test]
fn string_join_failures_release_resources_and_keep_runtime_usable() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "join-failure.kgr",
                r#"
fn trap() { [1].iter().map(|x| f"{x / 0}").join(","); }
fn structural() { val xs=["a"]; xs.iter().inspect(|x| { xs.push("b"); }).join(","); }
struct Forever {}
impl Iterator for Forever { type Item=String; fn next(self)->Option<String>{Some("x")} }
fn exhaust() { Forever{}.join(","); }
fn healthy()->i32 {42}
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
    for entry in ["trap", "structural", "exhaust"] {
        let mut options = context.clone();
        if entry == "exhaust" {
            options.resources.max_instruction_steps = Some(150);
        }
        let error = runtime.execute(&loaded, entry, &[], &options).unwrap_err();
        assert!(!format!("{error:?}").contains("UnsupportedExecution"));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(runtime.runtime().execution_root().is_none());
        assert!(!runtime.runtime().is_quarantined());
        assert_eq!(
            runtime
                .execute(&loaded, "healthy", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}

#[test]
fn map_interfaces_expose_ordered_readonly_snapshots() {
    execute(
        r#"
struct Item { var value: i32 }
struct One { val value: Item }
impl Iterable for One {
    type Item = (f64, Item);
    type Iter = Iter<(f64, Item)>;
    fn iter(self) -> Iter<(f64, Item)> { [(1.0, self.value)].iter() }
}
impl Map<f64, Item> for One {
    fn len(self) -> usize { 1usize }
    fn is_empty(self) -> bool { false }
    fn contains_key(self, key: f64) -> bool { key == 1.0 }
    fn get(self, key: f64) -> Option<Item> { if key == 1.0 { Some(self.value) } else { None } }
}
fn keys<K, V>(map: Map<K, V>) -> List<K> { map.keys() }
fn main() -> i32 {
    val storage = LinkedHashMap::from([("a", 1), ("b", 2)]);
    val map: Map<String, i32> = storage;
    val old = map.entries();
    std::debug::assert(keys(map)[1usize] == "b", "generic order");
    std::debug::assert(map.values()[0usize] == 1, "values");
    storage.clear();
    std::debug::assert(old.len() == 2usize, "independent slots");
    val object = Item { value: 7 };
    val custom: Map<f64, Item> = One { value: object };
    std::debug::assert(custom.keys()[0usize] == 1.0, "no hash bound");
    val values = custom.values();
    values[0usize].value = 42;
    std::debug::assert(object.value == 42, "shared payload");
    std::debug::assert(custom.entries().len() == 1usize, "custom entries");
    42
}
"#,
    );
}

#[test]
fn renamed_copy_has_no_legacy_alias_and_snapshots_are_readonly() {
    let engine = KagariEngine::default();
    for source in [
        "fn main() { val xs = [1]; xs.copy_from_slice([2]); }",
        "fn main() { val m: Map<i32,i32> = LinkedHashMap::new(); m.keys().push(1); }",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("rejected.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err()
        );
    }
}

#[test]
fn positional_defaults_use_selected_list_and_ordering_implementations() {
    execute(
        r#"
struct Sequence<T> {val items:ArrayList<T>}
impl<T> Iterable for Sequence<T> {type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{self.items.iter()}}
impl<T> Index<usize> for Sequence<T> {type Output=T;fn index(self,index:usize)->T{self.items[index]}}
impl<T> List<T> for Sequence<T> {fn len(self)->usize{self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn get(self,index:usize)->Option<T>{self.items.get(index)}}
fn positions<L:List<i32>>(source:L)->i32{std::debug::assert_eq(source.binary_search(22),Ok(1usize),"match");std::debug::assert_eq(source.binary_search(21),Err(1usize),"insertion");source.first().unwrap_or(0)+source.last().unwrap_or(0)}
fn main()->i32{val values=[20,22];val source=Sequence{items:values};val dynamic:List<i32> =source;std::debug::assert_eq(dynamic.binary_search(22),Ok(1usize),"dynamic index");std::debug::assert_eq(dynamic.first().unwrap_or(0)+dynamic.last().unwrap_or(0),42,"dynamic");std::debug::assert_eq(positions(source),42,"custom");val result=positions(values);values.push(0);result}
"#,
    );
}
