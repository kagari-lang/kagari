use super::{compile, compile_program};
use kagari_abi::{representation::ValueType, scalar::BuiltinType, types::AbiType};
use kagari_bytecode::module::BytecodeModuleSlot;
use kagari_runtime::{
    Runtime,
    value::{EnumTag, MapKey, Value},
    value_semantics::{format_value, script_equal},
};
use kagari_vm::vm::Vm;
use std::collections::HashSet;

#[test]
fn mark_sweep_traces_tuples_enum_payloads_and_cycles_without_retaining_unreachable_graphs() {
    let (mut vm, loaded) = compile(
        r#"
        struct Node { val edges: HashMap<i32, Node> }
        fn main() -> Option<(ArrayList<Node>,)> {
            val edges: HashMap<i32, Node> = HashMap::new();
            val node = Node { edges: edges };
            edges.insert(1, node);
            Some(([node],))
        }
        fn garbage() -> HashSet<i32> { val set: HashSet<i32> = HashSet::new(); set.insert(1); set }
    "#,
        None,
    );
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    let root = vm.runtime().root_value(Value::Tuple(vec![value])).unwrap();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 4);
    let live_units = vm.runtime().gc().stats().current_heap_units;
    assert_eq!(live_units, 8);
    vm.execute(&loaded, "garbage").unwrap();
    let collection = vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        (collection.reclaimed_objects, collection.live_objects),
        (1, 4)
    );
    assert_eq!(vm.runtime().gc().stats().current_heap_units, live_units);
    root.set(vm.runtime().gc(), Value::Unit).unwrap();
    let collection = vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        (collection.reclaimed_objects, collection.live_objects),
        (4, 0)
    );
    assert_eq!(vm.runtime().gc().stats().current_heap_units, 0);
}

#[test]
fn tracing_a_deep_heap_chain_uses_an_explicit_work_stack() {
    let (mut vm, loaded) = compile(
        r#"
        struct Node { val next: Option<Node> }
        fn main() -> Option<Node> {
            var head: Option<Node> = None;
            for index in 0..10000 { head = Some(Node { next: head }); }
            head
        }
    "#,
        None,
    );
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    let root = vm.runtime().root_value(value).unwrap();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 20_001);
    drop(root);
    assert_eq!(
        vm.runtime().collect_garbage().unwrap().reclaimed_objects,
        20_001
    );
}

#[test]
fn map_and_set_keys_keep_structural_payloads_and_identity_objects_alive() {
    let (mut vm, loaded) = compile(
        r#"
        fn main() -> (HashMap<Option<(ArrayList<i32>, String)>, i32>, HashSet<Option<(ArrayList<i32>, String)>>, bool) {
            val object = [42];
            val key = Some((object, "key"));
            val map: HashMap<Option<(ArrayList<i32>, String)>, i32> = HashMap::new();
            map.insert(key, 20);
            val set: HashSet<Option<(ArrayList<i32>, String)>> = HashSet::new();
            set.insert(key);
            object.push(99);
            val equal = Some((object, "key"));
            val found = match map.get(equal) { Some(value) => value == 20, None => false };
            (map, set, found && set.contains(equal))
        }
    "#,
        None,
    );
    let Value::Tuple(values) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("containers")
    };
    let [Value::Map(map), Value::Set(set), Value::Bool(found)] = values.as_slice() else {
        panic!("container handles")
    };
    assert!(
        *found,
        "selected Hash/Eq methods find structurally equal keys after identity mutation"
    );
    let (map, set) = (*map, *set);
    let runtime = vm.runtime();
    let key = runtime.gc().map_snapshot(map).unwrap()[0].0.clone();
    let Value::Enum(value) = key else {
        panic!("enum key")
    };
    let fields = runtime.gc().enum_snapshot(value).unwrap().fields;
    let [Value::Tuple(parts)] = fields.as_slice() else {
        panic!("tuple payload")
    };
    let [Value::Array(object), _] = parts.as_slice() else {
        panic!("identity payload")
    };
    let object = *object;
    let hash = MapKey::from_value(runtime.gc(), &key)
        .unwrap()
        .script_hash();
    let root = runtime
        .root_value(Value::Tuple(vec![Value::Map(map), Value::Set(set)]))
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 4);
    runtime.gc().array_push(object, Value::I32(101)).unwrap();
    assert_eq!(
        hash,
        MapKey::from_value(runtime.gc(), &key)
            .unwrap()
            .script_hash()
    );
    let equal = runtime
        .alloc_enum(
            EnumTag::OptionSome,
            vec![Value::Tuple(vec![
                Value::Array(object),
                Value::Str("key".into()),
            ])],
        )
        .unwrap();
    assert!(script_equal(runtime.gc(), &key, &Value::Enum(equal)).unwrap());
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    runtime.gc().map_clear(map).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 4);
    runtime.gc().set_clear(set).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 2);
    assert!(MapKey::from_value(runtime.gc(), &key).is_none());
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn invalid_identity_keys_are_rejected_without_container_modification() {
    let (mut vm, loaded) = compile(
        r#"
        fn main() -> (HashMap<i32, i32>, HashSet<i32>) {
            val map: HashMap<i32, i32> = HashMap::new(); map.insert(1, 42);
            val set: HashSet<i32> = HashSet::new(); set.insert(1); (map, set)
        }
    "#,
        None,
    );
    let Value::Tuple(values) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("containers")
    };
    let [Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("container handles")
    };
    let (map, set) = (*map, *set);
    let (foreign_vm, foreign_owner) = compile("fn main() {}", None);
    let object = foreign_vm
        .runtime()
        .alloc_array(&foreign_owner, AbiType::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    let runtime = vm.runtime();
    let before = runtime.resources().counters().allocation_units;
    assert!(
        runtime
            .gc()
            .map_insert(map, Value::Array(object), Value::I32(0))
            .is_err()
    );
    assert!(runtime.gc().set_insert(set, Value::Array(object)).is_err());
    assert!(
        runtime
            .gc()
            .map_insert(map, Value::F64(1.0), Value::I32(0))
            .is_err()
    );
    assert_eq!(runtime.resources().counters().allocation_units, before);
    assert_eq!(
        runtime.gc().map_snapshot(map),
        Some(vec![(Value::I32(1), Value::I32(42))])
    );
    assert_eq!(runtime.gc().set_snapshot(set), Some(vec![Value::I32(1)]));
}

#[test]
fn intrinsic_formatting_is_bounded_and_does_not_read_mutable_graphs() {
    let (mut vm, loaded) = compile(
        r#"
        struct Node { val items: ArrayList<Node> }
        fn main() -> ArrayList<Node> {
            val items: ArrayList<Node> = [];
            items.push(Node { items: items });
            items
        }
    "#,
        None,
    );
    let Value::Array(object) = vm.execute(&loaded, "main").unwrap().return_value else {
        panic!("array")
    };
    let runtime = vm.runtime();
    let preview = format_value(runtime.gc(), &Value::Array(object), true).unwrap();
    assert!(preview.starts_with("Array@"));
    assert_eq!(
        format_value(runtime.gc(), &Value::Str("a\nb".into()), true).unwrap(),
        "\"a\\nb\""
    );
    assert!(format_value(runtime.gc(), &Value::Str("x".repeat(1_048_577)), false).is_err());
    let mut value = Value::I32(42);
    for _ in 0..66 {
        value = Value::Tuple(vec![value]);
    }
    assert!(format_value(runtime.gc(), &value, true).is_err());
    assert_eq!(runtime.gc().array_len(object), Some(1));
}

#[test]
fn module_state_is_a_collection_root_until_its_version_is_reclaimed() {
    let mut module = compile_program(
        "fn init() -> ArrayList<i32> { [7] } fn main() -> i32 { 42 }",
        None,
    );
    module.modules[module.root.index()]
        .module_slots
        .push(BytecodeModuleSlot {
            name: "state".into(),
            ty: ValueType::HeapObject,
            mutable: true,
        });
    let mut runtime = Runtime::default();
    let program = module;
    let old = runtime.load_program("gc.kgr", program.clone()).unwrap();
    let mut vm = Vm::new(runtime);
    let array = vm
        .runtime()
        .alloc_array(
            &old,
            AbiType::Builtin(BuiltinType::I32),
            vec![Value::I32(7)],
        )
        .unwrap();
    {
        let mut instance = vm.runtime().module_instance_mut(&old).unwrap();
        instance.module_slots[0] = Value::Array(array);
    }
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 1);
    let new = vm.reload_program(&old, "gc.kgr", program).unwrap();
    let other = vm
        .runtime()
        .alloc_array(
            &new,
            AbiType::Builtin(BuiltinType::I32),
            vec![Value::I32(9)],
        )
        .unwrap();
    vm.runtime().module_instance_mut(&new).unwrap().module_slots[0] = Value::Array(other);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 2);
    let expected: HashSet<_> = old.members().map(|member| member.key()).collect();
    let reclaimed: HashSet<_> = vm
        .runtime()
        .modules()
        .collect_unreachable_epochs()
        .into_iter()
        .collect();
    assert_eq!(reclaimed, expected);
    assert_eq!(vm.runtime().collect_garbage().unwrap().reclaimed_objects, 1);
}

#[test]
fn rooted_data_keeps_type_metadata_without_retaining_obsolete_module_state() {
    let (mut vm, old) = compile(
        "struct Item { val value: i32 } fn main() -> ArrayList<Item> { [Item { value: 42 }] }",
        None,
    );
    let value = vm.execute(&old, "main").unwrap().return_value;
    let Value::Array(array) = value else {
        panic!("typed array")
    };
    let root = vm.runtime().root_value(value).unwrap();
    let replacement = vm
        .reload_program(
            &old,
            "boundary",
            compile_program("fn main() -> i32 { 99 }", None),
        )
        .unwrap();
    let expected: HashSet<_> = old.members().map(|member| member.key()).collect();
    let reclaimed: HashSet<_> = vm
        .runtime()
        .modules()
        .collect_unreachable_epochs()
        .into_iter()
        .collect();
    assert_eq!(reclaimed, expected);
    assert!(vm.runtime().validate_loaded_module(&old).is_err());
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 2);
    let item = vm.runtime().gc().array_get(array, 0).unwrap();
    vm.runtime().gc().array_push(array, item).unwrap();
    assert!(vm.runtime().gc().array_push(array, Value::I32(7)).is_err());
    assert_eq!(vm.runtime().gc().array_len(array), Some(2));
    assert_eq!(
        vm.execute(&replacement, "main").unwrap().return_value,
        Value::I32(99)
    );
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().reclaimed_objects, 2);
}
