// Share route preparation while retaining a fresh runtime per execution.
#[path = "support/execution.rs"]
mod execution;
use execution::Route;

use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::BytecodeInstruction,
    program::{identity, verify_program},
};
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::callable::witness::OperationWitness;
use kagari_embed::engine::{EngineConfig, KagariEngine};
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("standard-traits.kgr", source),
            Default::default(),
        )
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[Route::Source, Route::Artifact, Route::ArtifactJit],
        42,
    );
}

#[test]
fn inferred_common_interfaces_record_each_branch_conversion() {
    execute(
        r#"
trait X { fn x(self) -> i32; }
trait Y { fn y(self) -> i32; }
trait A: X + Y {}
trait B: X + Y {}
trait C: X {}
struct Item { val value: i32 }
struct Counter { var count: i32 }
impl X for Item { fn x(self) -> i32 { self.value } }
impl Y for Item { fn y(self) -> i32 { self.value } }
impl A for Item {}
impl B for Item {}
impl C for Item {}
fn a(counter: Counter) -> A { counter.count += 1; Item { value: 10 } }
fn b(counter: Counter) -> B { counter.count += 1; Item { value: 20 } }
fn c(counter: Counter) -> C { counter.count += 1; Item { value: 12 } }
fn choose(tag: i32, counter: Counter) -> i32 {
    val joined = match tag { 0 => a(counter), 1 => b(counter), _ => c(counter) };
    joined.x()
}
trait Read<T> { fn read(self) -> T; }
struct Left<T> { val item: T }
struct Right<T> { val item: T }
impl<T: Eq> Read<T> for Left<T> { fn read(self) -> T { self.item } }
impl<T: Eq> Read<T> for Right<T> { fn read(self) -> T { self.item } }
fn choose_read<T: Eq>(flag: bool, left: Left<T>, right: Right<T>) -> T {
    val joined = if flag { left } else { right };
    joined.read()
}
fn main() -> i32 {
    val counter = Counter { count: 0 };
    val total = choose(0, counter) + choose(1, counter) + choose(2, counter);
    if counter.count != 3 { return 0; }
    val left = Left { item: 7 };
    val right = Right { item: 9 };
    if choose_read(true, left, right) != 7 { return 0; }
    if choose_read(false, left, right) != 9 { return 0; }
    total
}
"#,
    );
}

#[test]
fn inferred_iterable_join_preserves_iterator_result_adaptation() {
    execute(
        r#"
trait Factory {
    fn later<T>(self, value: T) -> fn() -> Iterable<Item = T> {
        val items = Vec::from([value]);
        || items
    }
    fn consume<R: Reader<Iterable<Item = i32>>>(self, reader: R, value: Iterable<Item = i32>) -> i32 {
        reader.read(value)
    }
}
impl Factory for i32 {}
trait Reader<T> { fn read(self, value: T) -> i32; }
impl Reader<Iterable<Item = i32>> for i32 {
    fn read(self, value: Iterable<Item = i32>) -> i32 {
        var total = 0;
        for item in value { total += item; }
        total
    }
}
fn consume<R: Reader<Iterable<Item = i32>>>(reader: R, value: Iterable<Item = i32>) -> i32 {
    reader.read(value)
}
fn choose_iter(flag: bool, concrete: Vec<i32>, view: Iterable<Item = i32>) -> i32 {
    val joined = match flag { true => concrete, false => view };
    var total = 0;
    for item in joined { total += item; }
    total
}
fn main() -> i32 {
    if choose_iter(true, Vec::from([3, 4]), Vec::from([8])) != 7 { return 0; }
    if choose_iter(false, Vec::from([3, 4]), Vec::from([8])) != 8 { return 0; }
    val factory: Factory = 0;
    val later = factory.later(42);
    if consume(0, later()) != 42 { return 0; }
    factory.consume(0, later())
}
"#,
    );
}

#[test]
fn iterator_view_adaptation_rejects_forged_output_types() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "iterator-view-proof.kgr",
                "fn main() -> Iterable<Item = i32> { Vec::from([42]) }",
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    for wrong_result in [false, true] {
        let mut program = artifact.program.clone();
        let target = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::UpcastInterface { source, target, .. }
                    if source.declaration == target.declaration && source != target =>
                {
                    Some(target)
                }
                _ => None,
            })
            .expect("checked iterator-result adaptation");
        let result = target
            .associated_types
            .values_mut()
            .find(|value| matches!(value, Ty::Trait(_)))
            .expect("erased iterator result");
        if wrong_result {
            *result = Ty::Builtin(BuiltinType::I32);
        } else {
            let Ty::Trait(iterator) = result else {
                unreachable!()
            };
            *iterator.associated_types.values_mut().next().unwrap() = Ty::Builtin(BuiltinType::I64);
        }
        assert!(
            KbcArtifact::from_program(program, Default::default(), &context).is_err(),
            "forged iterator output (wrong result category: {wrong_result})"
        );
    }
}

#[test]
fn native_collection_views() {
    execute(
        r#"use std::collections::{HashMap, HashSet, List, Map, MutableList, MutableMap, MutableSet, Set};

fn size(xs: List<i32>) -> usize { xs.len() }
fn change(xs: MutableList<i32>) { xs.push(42); xs[0] += 6; }
fn main() -> i32 {
    val xs = Vec::from([1]);
    change(xs);
    val view: MutableList<i32> = xs;
    val read: std::collections::List<i32> = view;
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
        r#"use std::collections::{List, MutableList, Set, list_content_equal};
use std::iter::{CollectionCursor};

mod storage {
use std::iter::CollectionCursor;
use std::ops::Index;
pub struct Sequence { pub val items: Vec<i32> }
impl Index<usize> for Sequence {
    type Output = i32;
    fn index(self, index: usize) -> i32 { self.items[index] }
}
impl Iterable for Sequence {
    type Item = i32;
    type Iter = CollectionCursor<i32>;
    fn iter(self) -> CollectionCursor<i32> { self.items.iter() }
}
}
use self::storage::Sequence;
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
impl PartialEq for Singleton {
    fn eq(self, other: Self) -> bool { self.value == other.value }
}
impl Iterable for Singleton {
    type Item = f64;
    type Iter = CollectionCursor<f64>;
    fn iter(self) -> CollectionCursor<f64> { Vec::from([self.value]).iter() }
}
impl Set<f64> for Singleton {
    fn len(self) -> usize { 1usize }
    fn is_empty(self) -> bool { false }
    fn contains(self, value: f64) -> bool { self.value == value }
}
fn first<T>(xs: std::collections::List<T>) -> T { xs[0] }
fn size<C: List<i32>>(xs: C) -> usize { xs.len() }
fn main() -> i32 {
    val source = Sequence { items: Vec::from([7, 35]) };
    val view: std::collections::List<i32> = source;
    val copy = Vec::from([view[0], view[1]]);
    val copied_view: List<i32> = copy;
    val other_sequence: List<i32> = Sequence { items: Vec::from([7, 35]) };
    if !(view == view && view != other_sequence && other_sequence != view) { return 0; }
    if !(list_content_equal(view, copied_view) && list_content_equal(copied_view, view)) { return 0; }
    if !(list_content_equal(source, copy) && list_content_equal(copy, source)) { return 0; }
    val second_source = Sequence { items: Vec::from([7, 35]) };
    if !list_content_equal(source, second_source) { return 0; }
    if first(source) != 7 { return 0; }
    if !(!(view === copied_view) && view != copied_view && copied_view != view) { return 0; }
    val writable: MutableList<i32> = source;
    writable[0] -= 6;
    if !(copy[0] == 7 && view[0] == 1) { return 0; }
    if !(size(source) == 2usize && size(copy) == 2usize) { return 0; }
    val set: Set<f64> = Singleton { value: 0.5 };
    val same_set: Set<f64> = Singleton { value: 0.5 };
    val different_set: Set<f64> = Singleton { value: 1.0 };
    if !(set.contains(0.5) && set == same_set && same_set == set && set != different_set) { return 0; }
    first(copy) + copy[1]
}
"#,
    );
}

#[test]
fn hash_collection_interfaces_use_custom_key_protocols() {
    execute(
        r#"use std::collections::{HashMap, HashSet, Map, MutableMap, MutableSet};
use std::hash::{Hash};

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
    val set: MutableSet<Key> = HashSet::from([Key { value: 1 }, Key { value: 1 }]);
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
        "fn main() { val xs: std::collections::List<i32> = Vec::from([1]); xs[0] = 2; }",
        "use std::collections::{MutableList};\nfn main() { val xs: MutableList<i32> = Vec::from([1]); xs[true] = 2; }",
        "use std::collections::{MutableList};\nfn main() { val xs: MutableList<i32> = Vec::from([1]); xs[0i32] = 2; }",
        "use std::collections::{List, MutableList};\nfn main() { val xs: List<i32> = Vec::from([1]); val ys: MutableList<i32> = xs; }",
        "use std::collections::{Map};\nfn main() { val xs: Map<i32,i32> = Map::new(); }",
        "use std::collections::{HashSet, Set};\nfn main() { val xs: Set<f64> = HashSet::new(); }",
        "use std::collections::List; use std::hash::Hash; fn main() { val xs: List<i32> = Vec::from([1]); xs.hash(); }",
        "use std::collections::{List, HashSet}; fn main() { val keys: HashSet<List<i32>> = HashSet::new(); }",
    ] {
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("rejected.kgr", source), Default::default())
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn receiver_identity_survives_upcasts_branches_and_concrete_hash_storage() {
    execute(
        r#"use std::collections::{HashSet, List, MutableList};

struct Cell { var value: i32 }
fn readonly(xs: MutableList<Cell>) -> List<Cell> { xs }
fn main() -> i32 {
    val raw = Vec::from([Cell { value: 20 }]);
    val writable: MutableList<Cell> = raw;
    val read: List<Cell> = readonly(writable);
    read[0].value += 22;
    val joined = if true { raw } else { read };
    val selected = match 1 { 1 => raw, _ => read };
    if !(raw === read && writable == read && selected === joined) { return 0; }
    val copy = Vec::from([read[0]]);
    if !(raw == copy && raw.hash() == copy.hash()) { return 0; }
    val keys: HashSet<Vec<Cell>> = HashSet::new();
    keys.insert(raw); keys.insert(copy);
    if !(keys.len() == 1usize) { return 0; }
    raw[0].value
}
"#,
    );
}

#[test]
fn concrete_content_protocols_and_interface_helpers() {
    execute(
        r#"use std::collections::{HashMap, HashSet, List, Set, Map, list_content_equal, set_content_equal, map_content_equal};
use std::hash::Hash;
fn same<T: PartialEq>(a: T, b: T) -> bool { a == b }
fn lists_same<T: PartialEq>(a: Vec<T>, b: Vec<T>) -> bool { a == b }
fn as_list<T>(xs: Vec<T>) -> List<T> { xs }
trait Boxer { fn boxed<T>(self, xs: Vec<T>) -> List<T> { xs } }
impl Boxer for i32 {}
fn main() -> i32 {
    val a = Vec::from([1, 2]);
    val b = Vec::from([1, 2]);
    if !(a == b && same(a, b) && lists_same(a, b) && a.hash() == b.hash()) { return 1; }
    if a === b || a == Vec::from([2, 1]) { return 2; }
    val boxer: Boxer = 0;
    if !(boxer.boxed(a) == boxer.boxed(b)) { return 7; }
    val al: List<i32> = a;
    val bl: List<i32> = b;
    if !(al == bl && as_list(a) == as_list(b) && same(al, bl) && (al, 1) == (bl, 1) && list_content_equal(al, bl)) { return 3; }
    if !(list_content_equal(a, b) && list_content_equal(a, bl) && list_content_equal(al, b)) { return 8; }
    val s = HashSet::from([1, 2]);
    val t = HashSet::from([2, 1]);
    val sl: Set<i32> = s;
    val tl: Set<i32> = t;
    if !(s == t && same(s, t) && s.hash() == t.hash() && sl == tl && set_content_equal(sl, tl)) { return 4; }
    if !set_content_equal(s, t) { return 9; }
    val m: HashMap<i32, i32> = HashMap::new();
    m.insert(1, 10); m.insert(2, 20);
    val n: HashMap<i32, i32> = HashMap::new();
    n.insert(2, 20); n.insert(1, 10);
    val ml: Map<i32, i32> = m;
    val nl: Map<i32, i32> = n;
    if !(m == n && same(m, n) && m.hash() == n.hash() && ml == nl && map_content_equal(ml, nl)) { return 5; }
    if !map_content_equal(m, n) { return 10; }
    n.insert(1, 20); n.insert(2, 10);
    if m == n { return 6; }
    42
}
"#,
    );
}

#[test]
fn collection_content_composes_and_preserves_float_comparison() {
    execute(
        r#"use std::collections::{HashMap, HashSet, List, list_content_equal};
use std::hash::Hash;
fn main() -> i32 {
    val a = Vec::from([Vec::from([1, 2])]);
    val b = Vec::from([Vec::from([1, 2])]);
    if !(a == b && a.hash() == b.hash()) { return 1; }
    val keys: HashSet<Vec<Vec<i32>>> = HashSet::new();
    keys.insert(a);
    if !keys.contains(b) { return 2; }
    val nan = 0.0f32 / 0.0f32;
    val xs = Vec::from([nan]);
    val read: List<f32> = xs;
    if xs == xs || read == read || list_content_equal(read, read) { return 3; }
    if !(Vec::from([0.0f32]) == Vec::from([-0.0f32])) { return 4; }
    val m: HashMap<i32, f32> = HashMap::new();
    m.insert(1, nan);
    if m == m { return 5; }
    42
}
"#,
    );
    let engine = KagariEngine::default();
    for source in [
        "use std::hash::Hash; fn main() { Vec::from([0.0f32]).hash(); }",
        "use std::collections::HashSet; fn main() { val xs: HashSet<Vec<f32>> = HashSet::new(); }",
        "use std::collections::HashMap; use std::hash::Hash; fn main() { val m: HashMap<i32,f32> = HashMap::new(); m.hash(); }",
    ] {
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("rejected.kgr", source), Default::default())
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn map_helper_matches_entries_independently_of_lookup_policy_and_argument_order() {
    execute(
        r#"use std::collections::{Map, map_content_equal};
use std::iter::CollectionCursor;
struct Key { val tag: i32 }
impl PartialEq for Key { fn eq(self, other: Self) -> bool { true } }
impl Eq for Key {}
struct Value { val number: i32 }
impl PartialEq for Value {
    fn eq(self, other: Self) -> bool {
        self.number <= other.number + 1 && other.number <= self.number + 1
    }
}
struct Pairs { val entries: Vec<(Key, Value)> }
impl Iterable for Pairs {
    type Item = (Key, Value);
    type Iter = CollectionCursor<(Key, Value)>;
    fn iter(self) -> CollectionCursor<(Key, Value)> { self.entries.iter() }
}
impl Map<Key, Value> for Pairs {
    fn len(self) -> usize { self.entries.len() }
    fn is_empty(self) -> bool { self.entries.is_empty() }
    fn get(self, key: Key) -> Option<Value> {
        for (stored, value) in self.entries { if stored.tag == key.tag { return Some(value); } }
        None
    }
    fn contains_key(self, key: Key) -> bool {
        for (stored, value) in self.entries { if stored.tag == key.tag { return true; } }
        false
    }
}
fn main() -> i32 {
    val a: Map<Key, Value> = Pairs { entries: Vec::from([
        (Key { tag: 1 }, Value { number: 1 }),
        (Key { tag: 2 }, Value { number: 0 })
    ]) };
    val b: Map<Key, Value> = Pairs { entries: Vec::from([
        (Key { tag: 3 }, Value { number: 0 }),
        (Key { tag: 4 }, Value { number: 2 })
    ]) };
    val c: Map<Key, Value> = Pairs { entries: Vec::from([
        (Key { tag: 1 }, Value { number: 2 }),
        (Key { tag: 2 }, Value { number: 2 })
    ]) };
    val lookup = Key { tag: 3 };
    if a.contains_key(lookup) { return 1; }
    if !(map_content_equal(a, b) && map_content_equal(b, a)) { return 2; }
    if map_content_equal(a, c) || map_content_equal(c, a) { return 3; }
    42
}
"#,
    );
}

#[test]
fn receiver_comparison_evidence_is_checked_without_source() {
    let engine = KagariEngine::default();
    for source in [
        "use std::collections::List; fn main() -> bool { val xs: List<i32> = Vec::from([1]); xs == xs }",
        "use std::collections::List; trait Boxer {fn one<T>(self, value: T) -> List<T> {Vec::from([value])}} impl Boxer for i32 {} fn main() -> bool {val boxer: Boxer = 0; boxer.one(1) == boxer.one(1)}",
    ] {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("receiver-proof.kgr", source),
                Default::default(),
            )
            .unwrap();
        let context = DefinitionContext::new().unwrap();
        let artifact = artifact
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
        let definitions = identity::collect(
            &artifact.program.modules.iter().collect::<Vec<_>>(),
            &context,
        )
        .unwrap();
        verify_program(&artifact.program, &definitions).unwrap();
        for mutation in 0..4 {
            let mut program = artifact.program.clone();
            for table in program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.interface_tables)
            {
                let Some(comparison) = &mut table.comparison else {
                    continue;
                };
                match mutation {
                    0 => table.comparison = None,
                    1 => {
                        let OperationWitness::Selected(call) = &mut comparison.operation else {
                            panic!("concrete selection");
                        };
                        call.requirement.receiver = Ty::Builtin(BuiltinType::I32);
                    }
                    2 => {
                        let OperationWitness::Selected(call) = &mut comparison.operation else {
                            panic!("concrete selection");
                        };
                        call.signature.result = Ty::Builtin(BuiltinType::I32);
                    }
                    _ => {
                        comparison.operation = OperationWitness::Forward(Box::new(
                            comparison.operation.requirement().clone(),
                        ))
                    }
                }
            }
            assert!(
                KbcArtifact::from_program(program, Default::default(), &context).is_err(),
                "forged receiver comparison {mutation}"
            );
        }
    }
}

#[test]
fn shared_generic_boxing_selects_comparison_without_a_concrete_boxing_site() {
    execute(
        r#"
use std::collections::List;
fn forward<T>(xs: Vec<T>) -> List<T> { xs }
trait Boxer {
    fn boxed<T>(self, xs: Vec<T>) -> List<T> { forward(xs) }
    fn one<T>(self, value: T) -> List<T> { Vec::from([value]) }
    fn later<T>(self, value: T) -> fn() -> List<T> { || Vec::from([value]) }
}
impl Boxer for i32 {}
fn main() -> i32 {
    val boxer: Boxer = 0;
    val a = boxer.boxed(Vec::from([1]));
    val b = boxer.one(1);
    val later = boxer.later(1);
    if !(a == b && b == later()) { return 1; }
    val nan = boxer.one(0.0f32 / 0.0f32);
    if nan == nan { return 2; }
    if !(boxer.one(0.0f32) == boxer.one(-0.0f32)) { return 3; }
    42
}
"#,
    );
}
