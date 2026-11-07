use crate::tests::native_boundary::{compile, publish_unused_version};
use kagari_runtime::{
    error::RuntimeErrorKind,
    gc::{GcHeap, GcObjectKind, HeapObjectId},
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::CallContext,
        declarations::{FunctionDecl, MethodDecl},
        types::Type,
        views::SequenceMutHandle,
    },
    range::index_bound,
    reflection,
    value::{Value, ValueCategory},
    value_semantics::identity_equal,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::language::Protocol;
use std::ops::Bound;

#[test]
fn declared_sequence_layout_selects_contiguous_i32_even_when_empty() {
    let declarations = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::buffers",
        &declarations.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = builder.define_type("Buffer").unwrap();
    let item = declaration.type_parameter("T").unwrap();
    declaration.sequence_storage(&item).unwrap();
    let buffer = declaration.finish().unwrap();
    let ty = buffer.apply([Type::i32()]).unwrap();
    builder
        .implement(buffer.clone(), |group| {
            let receiver = group.receiver();
            group.inherent_impl(|methods| {
                let new =
                    methods.define_method(MethodDecl::static_method("new").returns(receiver))?;
                methods.bind(
                    new,
                    NativeBinding::new(vec![], buffer.codec(), |context| context.allocate_result()),
                )
            })?;
            group.trait_impl(
                declarations.protocol(Protocol::PartialEq).apply([])?,
                |methods| {
                    methods.bind(
                        "eq",
                        NativeBinding::new(
                            vec![buffer.codec(), buffer.codec()],
                            Codec::Exact(Type::bool()),
                            |context| {
                                identity_equal(
                                    context.heap(),
                                    &context.argument(0)?,
                                    &context.argument(1)?,
                                )
                                .map(Value::Bool)
                            },
                        ),
                    )
                },
            )?;
            group.trait_impl(declarations.eq().apply([])?, |_| Ok(()))?;
            group.trait_impl(declarations.hash().apply([])?, |methods| {
                methods.bind(
                    "hash",
                    // Collisions are intentional; equality still distinguishes buffer identities.
                    NativeBinding::new(vec![buffer.codec()], Codec::Exact(Type::i64()), |_| {
                        Ok(Value::I64(0))
                    }),
                )
            })
        })
        .unwrap();
    let push = builder
        .define_function(
            FunctionDecl::new("push")
                .parameter("buffer", ty.clone())
                .parameter("value", Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            push,
            NativeBinding::new(
                vec![buffer.codec(), Codec::Exact(Type::i32())],
                Codec::Exact(Type::unit()),
                |context| {
                    context.sequence_push(0, context.argument(1)?)?;
                    Ok(Value::Unit)
                },
            ),
        )
        .unwrap();
    let sum = builder
        .define_function(
            FunctionDecl::new("sum")
                .parameter("buffer", ty)
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            sum,
            |_context: &mut CallContext<'_>,
             mut buffer: SequenceMutHandle<'_>|
             -> NativeResult<i32> {
                assert!(
                    buffer
                        .read()
                        .with_slice::<u32, _>(|_| Ok(()))
                        .unwrap_err()
                        .message()
                        .contains("scalar layout")
                );
                buffer.with_slice_mut::<i32, _>(|values| {
                    values.reverse();
                    Ok(())
                })?;
                buffer
                    .read()
                    .with_slice::<i32, _>(|values| Ok(values.iter().copied().sum()))
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        "use std::collections::{HashMap};\nuse example::buffers::{Buffer, push, sum}; fn main() -> i32 { val buffer: Buffer<i32> = Buffer::new(); val other: Buffer<i32> = Buffer::new(); if buffer != buffer || buffer == other { return -2; } val keys = HashMap::new(); keys.insert(buffer, 42); if !keys.contains_key(buffer) || keys.contains_key(other) { return -3; } if sum(buffer) != 0 { return -1; } push(buffer, 20); push(buffer, 22); sum(buffer) }",
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn index_bounds_validate_the_applied_type_even_without_a_payload() {
    let (vm, loaded) = compile(
        r#"use core::ops::Bound;
        enum ForeignBound { Unbounded }
        pub fn valid() -> Bound<u64> { Bound::Unbounded }
                pub fn included() -> Bound<u64> { Bound::Included(18446744073709551615u64) }
                pub fn excluded() -> Bound<u64> { Bound::Excluded(4294967296u64) }
        pub fn wrong_argument() -> Bound<i32> { Bound::Unbounded }
        pub fn wrong_owner() -> ForeignBound { ForeignBound::Unbounded }
        "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let valid = vm.execute(&loaded, "valid").unwrap();
    assert_eq!(
        index_bound(
            vm.runtime().gc(),
            &valid
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result")
        )
        .unwrap(),
        Bound::Unbounded
    );
    for (name, expected) in [
        ("included", Bound::Included(u64::MAX)),
        ("excluded", Bound::Excluded(1_u64 << 32)),
    ] {
        let report = vm.execute(&loaded, name).unwrap();
        assert_eq!(
            index_bound(
                vm.runtime().gc(),
                &report.return_value.value(vm.runtime().gc()).unwrap()
            )
            .unwrap(),
            expected
        );
    }
    for name in ["wrong_argument", "wrong_owner"] {
        let invalid = vm.execute(&loaded, name).unwrap();
        assert_eq!(
            index_bound(
                vm.runtime().gc(),
                &invalid
                    .return_value
                    .value(vm.runtime().gc())
                    .expect("retained execution result")
            )
            .unwrap_err()
            .kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
}

#[test]
fn assigns_stable_object_identity_and_kind() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        struct Empty { val value: () }
        fn main() -> (Vec<i32>, HashMap<i32,i32>, HashSet<i32>, Empty) {
            (Vec::new(), HashMap::new(), HashSet::new(), Empty { value: () })
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let Value::Tuple(values) = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [
        Value::GcHandle(first),
        Value::Map(second),
        Value::Set(third),
        Value::Struct(fourth),
    ] = values.as_slice()
    else {
        panic!("handles")
    };
    let (first, second, third, fourth) = (*first, *second, *third, *fourth);
    let heap = vm.runtime().gc();
    for (value, expected) in [(Value::Map(second), "map"), (Value::Set(third), "set")] {
        let Value::Str(id) = reflection::type_of(heap, &value).unwrap() else {
            panic!("type name")
        };
        assert_eq!(&*heap.string(id).unwrap(), expected);
    }
    for value in [Value::Map(second), Value::Set(third)] {
        assert_eq!(value.category(), ValueCategory::ScriptOwned);
        assert!(value.is_default_heap_payload(heap));
    }
    assert_ne!(first, second);
    assert_ne!(second, third);
    assert_ne!(third, fourth);
    assert_eq!(first.index(), 0);
    assert_eq!(second.index(), 1);
    assert_eq!(third.index(), 2);
    assert_eq!(fourth.index(), 3);
    assert_eq!(heap.object_kind(first), Some(GcObjectKind::Native));
    assert_eq!(heap.object_kind(second), Some(GcObjectKind::Map));
    assert_eq!(heap.object_kind(third), Some(GcObjectKind::Set));
    assert_eq!(heap.object_kind(fourth), Some(GcObjectKind::Struct));
}

#[test]
fn hash_map_replaces_duplicates_and_accounts_units() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap};
fn main() -> HashMap<String,i32> { val map: HashMap<String,i32> = HashMap::new(); map.insert("b", 2); map.insert("a", 1); map.insert("b", 3); map }"#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let value = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    let Value::Map(map) = value else {
        panic!("container")
    };
    vm.runtime().collect_garbage().unwrap();
    let heap = vm.runtime().gc();
    assert_eq!(heap.map_len(map), Some(2));
    assert_eq!(
        sorted_map(heap, heap.map_snapshot(map).unwrap()),
        vec![("a".into(), Value::I32(1)), ("b".into(), Value::I32(3))]
    );
    let a = heap.alloc_string("a".into()).unwrap();
    let b = heap.alloc_string("b".into()).unwrap();
    let c = heap.alloc_string("c".into()).unwrap();
    let before = heap.stats().current_heap_units;
    heap.map_insert(map, c, Value::I32(4)).unwrap();
    assert_eq!(heap.stats().current_heap_units, before + 1);
    assert_eq!(heap.map_get(map, &c), Some(Value::I32(4)));
    heap.map_insert(map, a, Value::I32(9)).unwrap();
    assert_eq!(heap.stats().current_heap_units, before + 1);
    assert_eq!(
        sorted_map(heap, heap.map_snapshot(map).unwrap()),
        vec![
            ("a".into(), Value::I32(9)),
            ("b".into(), Value::I32(3)),
            ("c".into(), Value::I32(4))
        ]
    );
    assert_eq!(heap.map_remove(map, &b).unwrap(), Some(Value::I32(3)));
    assert_eq!(heap.stats().current_heap_units, before);
    heap.map_clear(map).unwrap();
    assert!(heap.map_snapshot(map).unwrap().is_empty());
    assert_eq!(heap.stats().current_heap_units, before - 2);
    // Clearing removes the map's ownership of its strings; sweeping reclaims them.
    vm.runtime().collect_garbage().unwrap();
    // Empty container (1) plus two one-byte module constants (2 units each).
    assert_eq!(heap.stats().current_heap_units, 5);
    drop(root);
    publish_unused_version(&vm, &_loaded_program);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(heap.stats().current_heap_units, 0);
}

#[test]
fn hash_set_replaces_duplicates_and_accounts_units() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashSet};
fn main() -> HashSet<String> { val set: HashSet<String> = HashSet::new(); set.insert("b"); set.insert("a"); set.insert("b"); set }"#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let value = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    let Value::Set(set) = value else {
        panic!("container")
    };
    vm.runtime().collect_garbage().unwrap();
    let heap = vm.runtime().gc();
    assert_eq!(heap.set_len(set), Some(2));
    assert_eq!(
        sorted_set(heap, heap.set_snapshot(set).unwrap()),
        vec!["a", "b"]
    );
    let a = heap.alloc_string("a".into()).unwrap();
    let b = heap.alloc_string("b".into()).unwrap();
    let c = heap.alloc_string("c".into()).unwrap();
    let before = heap.stats().current_heap_units;
    assert_eq!(heap.set_contains(set, &a), Some(true));
    assert_eq!(heap.set_insert(set, c), Ok(true));
    assert_eq!(heap.stats().current_heap_units, before + 1);
    assert_eq!(heap.set_insert(set, a), Ok(false));
    assert_eq!(heap.stats().current_heap_units, before + 1);
    assert_eq!(heap.set_remove(set, &b), Ok(true));
    assert_eq!(heap.stats().current_heap_units, before);
    heap.set_clear(set).unwrap();
    assert!(heap.set_snapshot(set).unwrap().is_empty());
    assert_eq!(heap.stats().current_heap_units, before - 2);
    vm.runtime().collect_garbage().unwrap();
    // Empty container (1) plus two one-byte module constants (2 units each).
    assert_eq!(heap.stats().current_heap_units, 5);
    drop(root);
    publish_unused_version(&vm, &_loaded_program);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(heap.stats().current_heap_units, 0);
}

#[test]
fn root_scanning_traces_only_gc_managed_boundaries() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        struct Record { val map: HashMap<String, Vec<i32>> }
        fn main() -> (Record, HashSet<String>) {
            val map: HashMap<String, Vec<i32>> = HashMap::new(); map.insert("leaf", Vec::from([1]));
            val set: HashSet<String> = HashSet::new(); set.insert("seen");
            (Record { map: map }, set)
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let Value::Tuple(values) = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [Value::Struct(record), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
    let (record, set) = (*record, *set);
    let heap = vm.runtime().gc();
    let schema = heap.struct_layout(record).unwrap();
    let Value::Map(map) = heap.struct_get_slot(record, &schema, 0).unwrap() else {
        panic!("map")
    };
    let Value::GcHandle(leaf) = heap
        .map_get(map, &heap.alloc_string("leaf".into()).unwrap())
        .unwrap()
    else {
        panic!("leaf")
    };
    let root = heap
        .root_value(
            heap.alloc_tuple(vec![Value::Struct(record), Value::Set(set), Value::Unit])
                .unwrap(),
        )
        .unwrap();

    let Value::Tuple(tuple) = root.value(heap).unwrap() else {
        panic!("root tuple")
    };
    let Value::Str(map_key) = heap.map_snapshot(map).unwrap()[0].0 else {
        panic!("map key")
    };
    let Value::Str(set_key) = heap.set_snapshot(set).unwrap()[0] else {
        panic!("set key")
    };
    assert_eq!(
        heap.trace_roots().unwrap(),
        vec![tuple, record, map, leaf, map_key, set, set_key]
    );

    root.set(heap, Value::GcHandle(leaf)).unwrap();
    assert_eq!(heap.trace_roots().unwrap(), vec![leaf]);

    assert_eq!(root.value(heap).unwrap(), Value::GcHandle(leaf));
    drop(root);
    assert_eq!(heap.trace_roots().unwrap(), Vec::<HeapObjectId>::new());
}

#[test]
fn root_scanning_handles_cycles_without_duplicate_identity() {
    let (vm, loaded) = compile(
        r#"
        struct Cycle { val array: Vec<Cycle> }
        fn main() -> Vec<Cycle> { val array: Vec<Cycle> = Vec::new(); array.push(Cycle { array: array }); array }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let Value::GcHandle(array) = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("array")
    };
    let heap = vm.runtime().gc();
    let Value::Struct(record) = heap.sequence_get(array, 0).unwrap() else {
        panic!("record")
    };
    let _root = heap.root_value(Value::GcHandle(array)).unwrap();

    assert_eq!(heap.trace_roots().unwrap(), vec![array, record]);
}

#[test]
fn removal_results_distinguish_absence_from_stale_handle_errors() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        fn main() -> (Vec<i32>, HashMap<i32,i32>, HashSet<i32>) { (Vec::new(), HashMap::new(), HashSet::new()) }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let Value::Tuple(values) = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [Value::GcHandle(array), Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
    let (array, map, set) = (*array, *map, *set);
    let heap = vm.runtime().gc();
    assert_eq!(heap.sequence_pop(array).unwrap(), None);
    assert_eq!(heap.sequence_remove(array, 0).unwrap(), None);
    assert_eq!(heap.map_remove(map, &Value::I32(1)).unwrap(), None);
    assert!(!heap.set_remove(set, &Value::I32(1)).unwrap());
    assert!(heap.map_remove(map, &Value::F64(1.0)).is_err());
    assert!(heap.set_remove(set, &Value::F64(1.0)).is_err());
    let before = heap.stats().current_heap_units;
    heap.sequence_clear(array).unwrap();
    heap.map_clear(map).unwrap();
    heap.set_clear(set).unwrap();
    assert_eq!(heap.stats().current_heap_units, before);
    vm.runtime().collect_garbage().unwrap();
    assert!(heap.sequence_pop(array).is_err());
    assert!(heap.sequence_remove(array, 0).is_err());
    assert!(heap.sequence_clear(array).is_err());
    assert!(heap.map_remove(map, &Value::I32(1)).is_err());
    assert!(heap.map_clear(map).is_err());
    assert!(heap.set_remove(set, &Value::I32(1)).is_err());
    assert!(heap.set_clear(set).is_err());
}

fn sorted_map(heap: &GcHeap, entries: Vec<(Value, Value)>) -> Vec<(String, Value)> {
    let mut entries: Vec<_> = entries
        .into_iter()
        .map(|(key, value)| {
            let Value::Str(id) = key else {
                panic!("string key")
            };
            (heap.string(id).unwrap().to_owned(), value)
        })
        .collect();
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    entries
}

fn sorted_set(heap: &GcHeap, entries: Vec<Value>) -> Vec<String> {
    let mut entries: Vec<_> = entries
        .into_iter()
        .map(|key| {
            let Value::Str(id) = key else {
                panic!("string key")
            };
            heap.string(id).unwrap().to_owned()
        })
        .collect();
    entries.sort();
    entries
}

#[test]
fn native_array_helpers_mutate_and_return_options() {
    let (vm, loaded) = compile(
        r#"
        fn main() -> (Vec<i32>, Option<i32>, Option<i32>, u64) {
            val array = Vec::from([1]); val length = array.len();
            array.push(3); array.insert(1, 2);
            val removed = array.remove(1); val missing = array.get(99);
            (array, removed, missing, length)
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let Value::Tuple(values) = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [
        Value::GcHandle(array),
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
        heap.sequence_snapshot(*array).unwrap(),
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
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap};

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
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let Value::Tuple(values) = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
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
        sorted_map(heap, heap.map_snapshot(*map).unwrap()),
        vec![("mp".into(), Value::I32(20))]
    );
    let key = heap.alloc_string("mp".into()).unwrap();
    let before = heap.stats();
    assert!(heap.map_insert(*map, key, Value::I64(20)).is_err());
    assert_eq!(heap.stats(), before);
}

#[test]
fn collection_revisions_distinguish_replacements_from_structural_writes() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        fn main() -> (Vec<i32>, HashMap<i32,i32>, HashSet<i32>) {
            val map: HashMap<i32,i32> = HashMap::new(); map.insert(1, 2);
            val set: HashSet<i32> = HashSet::new(); set.insert(1);
            (Vec::from([1]), map, set)
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let Value::Tuple(values) = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [Value::GcHandle(array), Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
    let (array, map, set) = (*array, *map, *set);
    let heap = vm.runtime().gc();
    let sources = [Value::GcHandle(array), Value::Map(map), Value::Set(set)];
    let revisions = sources.map(|value| heap.collection_revision(&value).unwrap());
    heap.sequence_set(array, 0, Value::I32(9)).unwrap();
    heap.map_insert(map, Value::I32(1), Value::I32(9)).unwrap();
    assert!(!heap.set_insert(set, Value::I32(1)).unwrap());
    for (source, revision) in sources.iter().zip(revisions) {
        heap.check_collection_revision(source, revision).unwrap();
    }
    heap.sequence_push(array, Value::I32(2)).unwrap();
    heap.map_insert(map, Value::I32(2), Value::I32(3)).unwrap();
    assert!(heap.set_insert(set, Value::I32(2)).unwrap());
    for (source, revision) in sources.iter().zip(revisions) {
        assert_eq!(
            heap.check_collection_revision(source, revision)
                .unwrap_err()
                .message(),
            "invalid or structurally modified iterator"
        );
    }
    assert_eq!(
        heap.sequence_snapshot(array).unwrap(),
        vec![Value::I32(9), Value::I32(2)]
    );
    assert_eq!(heap.map_len(map), Some(2));
    assert_eq!(heap.set_len(set), Some(2));
}

#[test]
fn native_map_and_set_allocations_update_resource_counters() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        fn main() -> (HashMap<String,i32>, HashSet<String>) {
            val map: HashMap<String,i32> = HashMap::new(); map.insert("hp", 100); map.insert("mp", 20);
            val set: HashSet<String> = HashSet::new(); set.insert("ready"); set.insert("visible");
            (map, set)
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let value = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    let Value::Tuple(values) = value else {
        panic!("tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
    let runtime = vm.runtime();
    assert_eq!(runtime.gc().map_len(*map), Some(2));
    assert_eq!(runtime.gc().set_len(*set), Some(2));
    let counters = runtime.resources().counters();

    assert_eq!(counters.current_heap_units, 29);
    assert_eq!(counters.peak_heap_units, 29);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 0);
    assert_eq!(runtime.resources().counters().current_heap_units, 29);

    drop(root);
}

#[test]
fn custom_set_absent_removal_does_not_invalidate_iteration() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashSet};
use std::hash::{Hash};

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
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    vm.execute(&loaded, ProgramEntry::Main).unwrap();
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.runtime().gc().allocated_objects(), 0);
}
