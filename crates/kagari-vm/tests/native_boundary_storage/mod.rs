use super::compile;
use kagari_runtime::{
    error::RuntimeErrorKind,
    gc::{GcObjectKind, HeapObjectId},
    reflection,
    value::{Value, ValueCategory},
};

#[test]
fn assigns_stable_object_identity_and_kind() {
    let (mut vm, loaded) = compile(
        r#"
        struct Empty { val value: () }
        fn main() -> (ArrayList<i32>, HashMap<i32,i32>, HashSet<i32>, Empty) {
            ([], HashMap::new(), HashSet::new(), Empty { value: () })
        }
    "#,
        None,
    );
    let Value::Tuple(values) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("tuple")
    };
    let [
        Value::Array(first),
        Value::Map(second),
        Value::Set(third),
        Value::Struct(fourth),
    ] = values.as_slice()
    else {
        panic!("handles")
    };
    let (first, second, third, fourth) = (*first, *second, *third, *fourth);
    let heap = vm.runtime().gc();
    assert_eq!(
        reflection::type_of(heap, &Value::Map(second)),
        Value::Str("map".into())
    );
    assert_eq!(
        reflection::type_of(heap, &Value::Set(third)),
        Value::Str("set".into())
    );
    for value in [Value::Map(second), Value::Set(third)] {
        assert_eq!(value.category(), ValueCategory::ScriptOwned);
        assert!(value.is_default_heap_payload());
    }
    assert_ne!(first, second);
    assert_ne!(second, third);
    assert_ne!(third, fourth);
    assert_eq!(first.index(), 0);
    assert_eq!(second.index(), 1);
    assert_eq!(third.index(), 2);
    assert_eq!(fourth.index(), 3);
    assert_eq!(heap.object_kind(first), Some(GcObjectKind::Array));
    assert_eq!(heap.object_kind(second), Some(GcObjectKind::Map));
    assert_eq!(heap.object_kind(third), Some(GcObjectKind::Set));
    assert_eq!(heap.object_kind(fourth), Some(GcObjectKind::Struct));
}

#[test]
fn hash_map_replaces_duplicates_and_accounts_units() {
    let (mut vm, loaded) = compile(
        r#"fn main() -> HashMap<String,i32> { val map: HashMap<String,i32> = HashMap::new(); map.insert("b", 2); map.insert("a", 1); map.insert("b", 3); map }"#,
        None,
    );
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    let root = vm.runtime().root_value(value.clone()).unwrap();
    let Value::Map(map) = value else {
        panic!("container")
    };
    vm.runtime().collect_garbage().unwrap();
    let heap = vm.runtime().gc();
    assert_eq!(heap.map_len(map), Some(2));
    assert_eq!(
        sorted_map(heap.map_snapshot(map).unwrap()),
        vec![
            (Value::Str("a".to_owned()), Value::I32(1)),
            (Value::Str("b".to_owned()), Value::I32(3)),
        ]
    );
    assert_eq!(heap.stats().current_heap_units, 3);

    heap.map_insert(map, Value::Str("c".to_owned()), Value::I32(4))
        .unwrap();
    assert_eq!(heap.stats().current_heap_units, 4);
    assert_eq!(
        heap.map_get(map, &Value::Str("c".to_owned())),
        Some(Value::I32(4))
    );

    heap.map_insert(map, Value::Str("a".to_owned()), Value::I32(9))
        .unwrap();
    assert_eq!(heap.stats().current_heap_units, 4);
    assert_eq!(
        sorted_map(heap.map_snapshot(map).unwrap()),
        vec![
            (Value::Str("a".to_owned()), Value::I32(9)),
            (Value::Str("b".to_owned()), Value::I32(3)),
            (Value::Str("c".to_owned()), Value::I32(4)),
        ]
    );

    assert_eq!(
        heap.map_remove(map, &Value::Str("b".to_owned())).unwrap(),
        Some(Value::I32(3))
    );
    assert_eq!(heap.stats().current_heap_units, 3);
    heap.map_clear(map).unwrap();
    assert_eq!(sorted_map(heap.map_snapshot(map).unwrap()), vec![]);
    assert_eq!(heap.stats().current_heap_units, 1);
    drop(root);
}

#[test]
fn hash_set_replaces_duplicates_and_accounts_units() {
    let (mut vm, loaded) = compile(
        r#"fn main() -> HashSet<String> { val set: HashSet<String> = HashSet::new(); set.insert("b"); set.insert("a"); set.insert("b"); set }"#,
        None,
    );
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    let root = vm.runtime().root_value(value.clone()).unwrap();
    let Value::Set(set) = value else {
        panic!("container")
    };
    vm.runtime().collect_garbage().unwrap();
    let heap = vm.runtime().gc();
    assert_eq!(heap.set_len(set), Some(2));
    assert_eq!(
        sorted_set(heap.set_snapshot(set).unwrap()),
        vec![Value::Str("a".to_owned()), Value::Str("b".to_owned())]
    );
    assert_eq!(heap.stats().current_heap_units, 3);
    assert_eq!(
        heap.set_contains(set, &Value::Str("a".to_owned())),
        Some(true)
    );

    assert_eq!(heap.set_insert(set, Value::Str("c".to_owned())), Ok(true));
    assert_eq!(heap.stats().current_heap_units, 4);
    assert_eq!(heap.set_insert(set, Value::Str("a".to_owned())), Ok(false));
    assert_eq!(heap.stats().current_heap_units, 4);
    assert_eq!(heap.set_remove(set, &Value::Str("b".to_owned())), Ok(true));
    assert_eq!(heap.stats().current_heap_units, 3);
    heap.set_clear(set).unwrap();
    assert_eq!(sorted_set(heap.set_snapshot(set).unwrap()), vec![]);
    assert_eq!(heap.stats().current_heap_units, 1);
    drop(root);
}

#[test]
fn root_scanning_traces_only_gc_managed_boundaries() {
    let (mut vm, loaded) = compile(
        r#"
        struct Record { val map: HashMap<String, ArrayList<i32>> }
        fn main() -> (Record, HashSet<String>) {
            val map: HashMap<String, ArrayList<i32>> = HashMap::new(); map.insert("leaf", [1]);
            val set: HashSet<String> = HashSet::new(); set.insert("seen");
            (Record { map: map }, set)
        }
    "#,
        None,
    );
    let Value::Tuple(values) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("tuple")
    };
    let [Value::Struct(record), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
    let (record, set) = (*record, *set);
    let heap = vm.runtime().gc();
    let schema = heap.struct_layout(record).unwrap();
    let Value::Map(map) = heap.struct_get_slot(record, &schema, 0).unwrap() else {
        panic!("map")
    };
    let Value::Array(leaf) = heap.map_get(map, &Value::Str("leaf".into())).unwrap() else {
        panic!("leaf")
    };
    let root = heap
        .root_value(Value::Tuple(vec![
            Value::Struct(record),
            Value::Set(set),
            Value::Unit,
        ]))
        .unwrap();

    assert_eq!(heap.trace_roots().unwrap(), vec![record, map, leaf, set]);

    root.set(heap, Value::GcHandle(leaf)).unwrap();
    assert_eq!(heap.trace_roots().unwrap(), vec![leaf]);

    assert_eq!(root.value(), Value::GcHandle(leaf));
    drop(root);
    assert_eq!(heap.trace_roots().unwrap(), Vec::<HeapObjectId>::new());
}

#[test]
fn root_scanning_handles_cycles_without_duplicate_identity() {
    let (mut vm, loaded) = compile(
        r#"
        struct Cycle { val array: ArrayList<Cycle> }
        fn main() -> ArrayList<Cycle> { val array: ArrayList<Cycle> = []; array.push(Cycle { array: array }); array }
    "#,
        None,
    );
    let Value::Array(array) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("array")
    };
    let heap = vm.runtime().gc();
    let Value::Struct(record) = heap.array_get(array, 0).unwrap() else {
        panic!("record")
    };
    let _root = heap.root_value(Value::Array(array)).unwrap();

    assert_eq!(heap.trace_roots().unwrap(), vec![array, record]);
}
#[test]
fn removal_results_distinguish_absence_from_iteration_and_stale_handle_errors() {
    let (mut vm, loaded) = compile(
        r#"
        fn main() -> (ArrayList<i32>, HashMap<i32,i32>, HashSet<i32>) { ([], HashMap::new(), HashSet::new()) }
    "#,
        None,
    );
    let Value::Tuple(values) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("tuple")
    };
    let [Value::Array(array), Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
    let (array, map, set) = (*array, *map, *set);
    let heap = vm.runtime().gc();
    assert_eq!(heap.array_pop(array).unwrap(), None);
    assert_eq!(heap.array_remove(array, 0).unwrap(), None);
    assert_eq!(heap.map_remove(map, &Value::I32(1)).unwrap(), None);
    assert!(!heap.set_remove(set, &Value::I32(1)).unwrap());
    assert!(heap.map_remove(map, &Value::F64(1.0)).is_err());
    assert!(heap.set_remove(set, &Value::F64(1.0)).is_err());
    let guards = [Value::Array(array), Value::Map(map), Value::Set(set)]
        .map(|value| heap.begin_collection_iteration(&value).unwrap());
    let before = heap.stats().current_heap_units;
    for result in [
        heap.array_pop(array).map(|_| ()),
        heap.array_remove(array, 0).map(|_| ()),
        heap.array_clear(array),
        heap.map_remove(map, &Value::I32(1)).map(|_| ()),
        heap.map_clear(map),
        heap.set_remove(set, &Value::I32(1)).map(|_| ()),
        heap.set_clear(set),
    ] {
        let error = result.unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        assert_eq!(error.message(), "structural modification during iteration");
    }
    assert_eq!(heap.stats().current_heap_units, before);
    drop(guards);
    heap.array_clear(array).unwrap();
    heap.map_clear(map).unwrap();
    heap.set_clear(set).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert!(heap.array_pop(array).is_err());
    assert!(heap.array_remove(array, 0).is_err());
    assert!(heap.array_clear(array).is_err());
    assert!(heap.map_remove(map, &Value::I32(1)).is_err());
    assert!(heap.map_clear(map).is_err());
    assert!(heap.set_remove(set, &Value::I32(1)).is_err());
    assert!(heap.set_clear(set).is_err());
}
fn sorted_map(mut entries: Vec<(Value, Value)>) -> Vec<(Value, Value)> {
    entries.sort_by(|(a, _), (b, _)| match (a, b) {
        (Value::Str(a), Value::Str(b)) => a.cmp(b),
        _ => panic!("string keys"),
    });
    entries
}
fn sorted_set(mut entries: Vec<Value>) -> Vec<Value> {
    entries.sort_by(|a, b| match (a, b) {
        (Value::Str(a), Value::Str(b)) => a.cmp(b),
        _ => panic!("string keys"),
    });
    entries
}

#[test]
fn native_array_helpers_mutate_and_return_options() {
    let (mut vm, loaded) = compile(
        r#"
        fn main() -> (ArrayList<i32>, Option<i32>, Option<i32>, usize) {
            val array = [1]; val length = array.len();
            array.push(3); array.insert(1, 2);
            val removed = array.remove(1); val missing = array.get(99);
            (array, removed, missing, length)
        }
    "#,
        None,
    );
    let Value::Tuple(values) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("tuple")
    };
    let [
        Value::Array(array),
        Value::Enum(removed),
        Value::Enum(missing),
        Value::U64(length),
    ] = values.as_slice()
    else {
        panic!("results")
    };
    let heap = vm.runtime().gc();
    assert_eq!(*length, 1);
    assert_eq!(
        heap.array_snapshot(*array).unwrap(),
        vec![Value::I32(1), Value::I32(3)]
    );
    let removed = heap.enum_snapshot(*removed).unwrap();
    assert_eq!(removed.tag.variant_name(), "Some");
    assert_eq!(removed.fields, vec![Value::I32(2)]);
    let missing = heap.enum_snapshot(*missing).unwrap();
    assert_eq!(missing.tag.variant_name(), "None");
    assert!(missing.fields.is_empty());
}

#[test]
fn native_map_helpers_return_options_and_keep_declared_types() {
    let (mut vm, loaded) = compile(
        r#"
        fn main() -> (HashMap<String,i32>, Option<i32>, Option<i32>, bool) {
            val map: HashMap<String,i32> = HashMap::new();
            map.insert("hp", 100); map.insert("mp", 20);
            val contains = map.contains_key("hp");
            val removed = map.remove("hp"); val missing = map.get("hp");
            (map, removed, missing, contains)
        }
    "#,
        None,
    );
    let Value::Tuple(values) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("tuple")
    };
    let [
        Value::Map(map),
        Value::Enum(removed),
        Value::Enum(missing),
        Value::Bool(contains),
    ] = values.as_slice()
    else {
        panic!("results")
    };
    let heap = vm.runtime().gc();
    assert!(*contains);
    let removed = heap.enum_snapshot(*removed).unwrap();
    assert_eq!(removed.tag.variant_name(), "Some");
    assert_eq!(removed.fields, vec![Value::I32(100)]);
    assert_eq!(
        heap.enum_snapshot(*missing).unwrap().tag.variant_name(),
        "None"
    );
    assert_eq!(
        heap.map_snapshot(*map).unwrap(),
        vec![(Value::Str("mp".into()), Value::I32(20))]
    );
    let before = heap.stats();
    assert!(
        heap.map_insert(*map, Value::Str("mp".into()), Value::I64(20))
            .is_err()
    );
    assert_eq!(heap.stats(), before);
}

#[test]
fn collection_iteration_rejects_structural_alias_writes_before_allocation() {
    let (mut vm, loaded) = compile(
        r#"
        fn main() -> (ArrayList<i32>, HashMap<i32,i32>, HashSet<i32>) {
            val map: HashMap<i32,i32> = HashMap::new(); map.insert(1, 2);
            val set: HashSet<i32> = HashSet::new(); set.insert(1);
            ([1], map, set)
        }
    "#,
        None,
    );
    let Value::Tuple(values) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("tuple")
    };
    let [Value::Array(array), Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
    let (array, map, set) = (*array, *map, *set);
    let heap = vm.runtime().gc();
    let guards = [Value::Array(array), Value::Map(map), Value::Set(set)]
        .map(|value| heap.begin_collection_iteration(&value).unwrap());
    let before = heap.stats();
    for result in [
        heap.array_push(array, Value::I32(2)),
        heap.array_pop(array).map(|_| ()),
        heap.array_insert(array, 0, Value::I32(2)),
        heap.array_remove(array, 0).map(|_| ()),
        heap.array_clear(array),
        heap.map_insert(map, Value::I32(3), Value::I32(4)),
        heap.map_remove(map, &Value::I32(1)).map(|_| ()),
        heap.map_clear(map),
        heap.set_insert(set, Value::I32(2)).map(|_| ()),
        heap.set_remove(set, &Value::I32(1)).map(|_| ()),
        heap.set_clear(set),
    ] {
        assert_eq!(
            result.unwrap_err().message(),
            "structural modification during iteration"
        );
        assert_eq!(heap.stats(), before);
        assert_eq!(heap.array_snapshot(array).unwrap(), vec![Value::I32(1)]);
        assert_eq!(
            heap.map_snapshot(map).unwrap(),
            vec![(Value::I32(1), Value::I32(2))]
        );
        assert_eq!(heap.set_snapshot(set).unwrap(), vec![Value::I32(1)]);
    }
    heap.array_set(array, 0, Value::I32(9)).unwrap();
    let nested = heap
        .begin_collection_iteration(&Value::Array(array))
        .unwrap();
    drop(nested);
    assert!(heap.array_push(array, Value::I32(2)).is_err());
    heap.map_insert(map, Value::I32(1), Value::I32(9)).unwrap();
    assert!(!heap.set_insert(set, Value::I32(1)).unwrap());
    drop(guards);
    heap.array_push(array, Value::I32(2)).unwrap();
}

#[test]
fn native_map_and_set_allocations_update_resource_counters() {
    let (mut vm, loaded) = compile(
        r#"
        fn main() -> (HashMap<String,i32>, HashSet<String>) {
            val map: HashMap<String,i32> = HashMap::new(); map.insert("hp", 100); map.insert("mp", 20);
            val set: HashSet<String> = HashSet::new(); set.insert("ready"); set.insert("visible");
            (map, set)
        }
    "#,
        None,
    );
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    let root = vm.runtime().root_value(value.clone()).unwrap();
    let Value::Tuple(values) = value else {
        panic!("tuple")
    };
    let [Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
    let runtime = vm.runtime();
    assert_eq!(runtime.gc().map_len(*map), Some(2));
    assert_eq!(runtime.gc().set_len(*set), Some(2));
    let counters = runtime.resources().counters();

    assert_eq!(counters.current_heap_units, 6);
    assert_eq!(counters.peak_heap_units, 6);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 0);
    assert_eq!(runtime.resources().counters().current_heap_units, 6);

    drop(root);
}

#[test]
fn custom_set_removal_rejects_iteration_even_when_the_key_is_absent() {
    let (mut vm, loaded) = compile(
        r#"
        struct Key { val number: i32 }
        impl PartialEq for Key { fn eq(self, other: Key) -> bool { self.number == other.number } }
        impl Eq for Key {}
        impl Hash for Key { fn hash(self) -> i64 { 7 } }
        fn main() {
            val set: HashSet<Key> = HashSet::new(); set.insert(Key { number: 1 });
            for item in set { set.remove(Key { number: 2 }); }
        }
    "#,
        None,
    );
    let error = vm.execute(&loaded, "main").unwrap_err();
    assert!(
        format!("{error:?}").contains("structural modification during iteration"),
        "{error:?}"
    );
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.runtime().gc().allocated_objects(), 0);
}
