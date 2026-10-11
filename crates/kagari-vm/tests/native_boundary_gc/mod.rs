mod payload_failures;
mod program_cycles;
use super::{compile, compile_program};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::ModuleSlot, module::BytecodeModuleSlot};
use kagari_runtime::{
    Runtime,
    value::{MapKey, Value},
    value_semantics::{format_value, script_equal},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use kagari_vm::vm::Vm;
use std::collections::HashSet;

#[test]
fn mark_sweep_traces_tuples_enum_payloads_and_cycles_without_retaining_unreachable_graphs() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        struct Node { val edges: HashMap<i32, Node> }
        fn main() -> Option<(Vec<Node>,)> {
            val edges: HashMap<i32, Node> = HashMap::new();
            val node = Node { edges: edges };
            edges.insert(1, node);
            Some((Vec::from([node]),))
        }
        fn garbage() -> HashSet<i32> { val set: HashSet<i32> = HashSet::new(); set.insert(1); set }
    "#,
        None,
    );
    let value = vm
        .execute(&loaded, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm
        .runtime()
        .root_value(vm.runtime().gc().alloc_tuple(vec![value]).unwrap())
        .unwrap();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 6);
    let live_units = vm.runtime().gc().stats().current_heap_units;
    assert_eq!(live_units, 12);
    vm.execute(&loaded, "garbage").unwrap();
    let collection = vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        (collection.reclaimed_objects, collection.live_objects),
        (1, 6)
    );
    assert_eq!(vm.runtime().gc().stats().current_heap_units, live_units);
    root.set(vm.runtime().gc(), Value::Unit).unwrap();
    let collection = vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        (collection.reclaimed_objects, collection.live_objects),
        (6, 0)
    );
    assert_eq!(vm.runtime().gc().stats().current_heap_units, 0);
}

#[test]
fn tracing_a_deep_heap_chain_uses_an_explicit_work_stack() {
    let (vm, loaded) = compile(
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
    let value = vm
        .execute(&loaded, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
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
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        fn main() -> (HashMap<Option<(Vec<i32>, String)>, i32>, HashSet<Option<(Vec<i32>, String)>>, bool) {
            val object = Vec::from([42]);
            val key = Some((object, "key"));
            val map: HashMap<Option<(Vec<i32>, String)>, i32> = HashMap::new();
            map.insert(key, 20);
            val set: HashSet<Option<(Vec<i32>, String)>> = HashSet::new();
            set.insert(key);
            object.push(99);
            val equal = Some((object, "key"));
            val found = match map.get(equal) { Some(value) => value == 20, None => false };
            (map, set, found && set.contains(equal))
        }
    "#,
        None,
    );
    let Value::Tuple(values) = vm
        .execute(&loaded, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("containers")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [Value::Map(map), Value::Set(set), Value::Bool(found)] = values.as_slice() else {
        panic!("container handles")
    };
    assert!(
        *found,
        "selected Hash/Eq methods find structurally equal keys after identity mutation"
    );
    let (map, set) = (*map, *set);
    let runtime = vm.runtime();
    let key = runtime.gc().map_snapshot(map).unwrap()[0].0;
    let Value::Enum(value) = key else {
        panic!("enum key")
    };
    let fields = runtime.gc().enum_snapshot(value).unwrap().fields;
    let [Value::Tuple(parts)] = fields.as_slice() else {
        panic!("tuple payload")
    };
    let parts = runtime.gc().tuple(*parts).unwrap().to_vec();
    let [Value::GcHandle(object), _] = parts.as_slice() else {
        panic!("identity payload")
    };
    let object = *object;
    let hash = MapKey::from_value(runtime.gc(), &key)
        .unwrap()
        .script_hash();
    let root = runtime
        .root_value(
            vm.runtime()
                .gc()
                .alloc_tuple(vec![Value::Map(map), Value::Set(set)])
                .unwrap(),
        )
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 7);
    runtime.gc().sequence_push(object, Value::I32(101)).unwrap();
    assert_eq!(
        hash,
        MapKey::from_value(runtime.gc(), &key)
            .unwrap()
            .script_hash()
    );
    let equal = runtime
        .alloc_enum(
            runtime
                .gc()
                .enum_snapshot(match key {
                    Value::Enum(id) => id,
                    _ => panic!("enum key"),
                })
                .unwrap()
                .tag,
            vec![
                vm.runtime()
                    .gc()
                    .alloc_tuple(vec![
                        Value::GcHandle(object),
                        vm.runtime().gc().alloc_string("key".into()).unwrap(),
                    ])
                    .unwrap(),
            ],
        )
        .unwrap();
    assert!(script_equal(runtime.gc(), &key, &Value::Enum(equal)).unwrap());
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 3);
    runtime.gc().map_clear(map).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 7);
    runtime.gc().set_clear(set).unwrap();
    // The original string is still owned by the published constant cache.
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 3);
    assert!(MapKey::from_value(runtime.gc(), &key).is_none());
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    let candidate = runtime
        .stage_reload_verified_program(
            &loaded,
            loaded.name.clone(),
            loaded.verified_program().clone(),
        )
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn invalid_identity_keys_are_rejected_without_container_modification() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        fn main() -> (HashMap<i32, i32>, HashSet<i32>) {
            val map: HashMap<i32, i32> = HashMap::new(); map.insert(1, 42);
            val set: HashSet<i32> = HashSet::new(); set.insert(1); (map, set)
        }
    "#,
        None,
    );
    let Value::Tuple(values) = vm
        .execute(&loaded, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("containers")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("container handles")
    };
    let (map, set) = (*map, *set);
    let (foreign_vm, foreign_owner) = compile("fn main() {}", None);
    let object = foreign_vm
        .runtime()
        .alloc_array(&foreign_owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    let runtime = vm.runtime();

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

    assert_eq!(
        runtime.gc().map_snapshot(map),
        Some(vec![(Value::I32(1), Value::I32(42))])
    );
    assert_eq!(runtime.gc().set_snapshot(set), Some(vec![Value::I32(1)]));
}

#[test]
fn intrinsic_formatting_is_bounded_and_does_not_read_mutable_graphs() {
    let (vm, loaded) = compile(
        r#"
        struct Node { val items: Vec<Node> }
        fn main() -> Vec<Node> {
            val items: Vec<Node> = Vec::from([]);
            items.push(Node { items: items });
            items
        }
    "#,
        None,
    );
    let Value::GcHandle(object) = vm
        .execute(&loaded, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("array")
    };
    let runtime = vm.runtime();
    let preview = format_value(runtime.gc(), &Value::GcHandle(object), true).unwrap();
    assert!(preview.starts_with("Vec@"));
    assert_eq!(
        format_value(
            runtime.gc(),
            &vm.runtime().gc().alloc_string("a\nb".into()).unwrap(),
            true
        )
        .unwrap(),
        "\"a\\nb\""
    );
    assert!(
        format_value(
            runtime.gc(),
            &vm.runtime()
                .gc()
                .alloc_string("x".repeat(1_048_577))
                .unwrap(),
            false
        )
        .is_err()
    );
    let mut value = Value::I32(42);
    for _ in 0..66 {
        value = vm.runtime().gc().alloc_tuple(vec![value]).unwrap();
    }
    assert!(format_value(runtime.gc(), &value, true).is_err());
    assert_eq!(runtime.gc().sequence_len(object), Some(1));
}

#[test]
fn module_state_is_a_collection_root_until_its_version_is_reclaimed() {
    let mut module = compile_program(
        "fn init() -> Vec<i32> { Vec::from([7]) } fn main() -> i32 { 42 }",
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
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let program = module;
    let old = runtime.load_program("gc.kgr", program.clone()).unwrap();
    let vm = Vm::new(runtime);
    let array = vm
        .runtime()
        .alloc_array(&old, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    vm.runtime()
        .write_module_slot(&old, ModuleSlot::new(0), Value::Array(array))
        .unwrap();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 1);
    let new = vm.reload_program(&old, "gc.kgr", program).unwrap();
    let other = vm
        .runtime()
        .alloc_array(&new, Ty::Builtin(BuiltinType::I32), vec![Value::I32(9)])
        .unwrap();
    vm.runtime()
        .write_module_slot(&new, ModuleSlot::new(0), Value::Array(other))
        .unwrap();
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 1);
    assert_eq!(collected.reclaimed_objects, 1);
    let expected: HashSet<_> = old.members().map(|member| member.key()).collect();
    let reclaimed: HashSet<_> = collected.reclaimed_modules.into_iter().collect();
    assert_eq!(reclaimed, expected);
    assert!(vm.runtime().gc().array_len(array).is_none());
    assert_eq!(vm.runtime().gc().array_get(other, 0), Some(Value::I32(9)));
}

#[test]
fn rooted_data_keeps_type_metadata_without_retaining_obsolete_module_state() {
    let (vm, old) = compile(
        "struct Item { val value: i32 } fn main() -> Vec<Item> { Vec::from([Item { value: 42 }]) }",
        None,
    );
    let value = vm
        .execute(&old, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let Value::GcHandle(array) = value else {
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
        .collect_garbage()
        .unwrap()
        .reclaimed_modules
        .into_iter()
        .collect();
    assert_eq!(reclaimed, expected);
    assert!(vm.runtime().validate_loaded_module(&old).is_err());
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 2);
    let item = vm.runtime().gc().sequence_get(array, 0).unwrap();
    vm.runtime().gc().sequence_push(array, item).unwrap();
    assert!(
        vm.runtime()
            .gc()
            .sequence_push(array, Value::I32(7))
            .is_err()
    );
    assert_eq!(vm.runtime().gc().sequence_len(array), Some(2));
    assert_eq!(
        vm.execute(&replacement, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(99)
    );
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().reclaimed_objects, 2);
}

#[test]
fn recursive_element_contracts_reject_changed_nested_layouts_after_reload() {
    let source = r#"
        struct Inner { val value: i32 }
        struct Node { var next: Option<Node>, val value: Inner }
        fn main() -> Vec<Node> {
            val node = Node { next: None, value: Inner { value: 42 } };
            node.next = Some(node);
            Vec::from([node])
        }
    "#;
    let (vm, old) = compile(source, None);
    let old_value = vm
        .execute(&old, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let old_root = vm.runtime().root_value(old_value).unwrap();
    let replacement = source
        .replace("val value: i32", "val value: i32, val extra: bool")
        .replace("value: 42 }", "value: 42, extra: true }");
    let current = vm
        .reload_program(&old, "boundary", compile_program(&replacement, None))
        .unwrap();
    let new_value = vm
        .execute(&current, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let new_root = vm.runtime().root_value(new_value).unwrap();
    let (Value::GcHandle(old_array), Value::GcHandle(new_array)) = (old_value, new_value) else {
        panic!("typed arrays");
    };
    let reclaimed = vm.runtime().collect_garbage().unwrap().reclaimed_modules;
    assert!(
        old.members()
            .all(|member| reclaimed.contains(&member.key()))
    );
    let old_node = vm.runtime().gc().sequence_get(old_array, 0).unwrap();
    let new_node = vm.runtime().gc().sequence_get(new_array, 0).unwrap();
    assert!(
        vm.runtime()
            .gc()
            .sequence_push(old_array, new_node)
            .is_err()
    );
    assert!(
        vm.runtime()
            .gc()
            .sequence_push(new_array, old_node)
            .is_err()
    );
    assert_eq!(vm.runtime().gc().sequence_len(old_array), Some(1));
    assert_eq!(vm.runtime().gc().sequence_len(new_array), Some(1));
    vm.runtime()
        .gc()
        .sequence_push(old_array, old_node)
        .unwrap();
    vm.runtime()
        .gc()
        .sequence_push(new_array, new_node)
        .unwrap();
    drop((old_root, new_root));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}
