mod payload_failures;
mod program_cycles;
use super::{compile, compile_program};
use crate::vm::owned::DriveResult;
use kagari_runtime::{
    error::RuntimeError,
    gc::roots::RootedValue,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        types::Type,
    },
    value::Value,
    value_semantics::format_value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    collections::HashSet,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

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
        pub fn garbage() -> HashSet<i32> { val set: HashSet<i32> = HashSet::new(); set.insert(1); set }
    "#,
        None,
    );
    let value = vm
        .execute(&loaded, ProgramEntry::Main)
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
        .execute(&loaded, ProgramEntry::Main)
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
        include_str!("../../../../tests/fixtures/native/structural-and-identity-hash-keys.kgr"),
        None,
    );
    let Value::Tuple(values) = vm
        .execute(&loaded, ProgramEntry::Main)
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
    let root = runtime
        .root_value(
            runtime
                .gc()
                .alloc_tuple(vec![Value::Map(map), Value::Set(set)])
                .unwrap(),
        )
        .unwrap();
    let call = |entry: &str, arguments: &[Value]| {
        let execution = vm
            .start(&loaded, entry, arguments, Default::default())
            .unwrap();
        loop {
            match vm
                .drive(&execution, NonZeroUsize::new(1024).unwrap())
                .unwrap()
            {
                DriveResult::Runnable => {}
                DriveResult::Waiting => panic!("synchronous key protocol suspended"),
                DriveResult::Complete(result) => {
                    break result.unwrap().value(runtime.gc()).unwrap();
                }
            }
        }
    };
    let key = runtime.gc().map_snapshot(map).unwrap()[0].0;
    let Value::Enum(value) = key else {
        panic!("enum key")
    };
    let fields = runtime.gc().enum_snapshot(value).unwrap().fields;
    let [Value::Tuple(parts)] = fields.as_slice() else {
        panic!("tuple payload")
    };
    let parts = runtime.gc().tuple(*parts).unwrap().to_vec();
    let [Value::Struct(object), _] = parts.as_slice() else {
        panic!("identity payload")
    };
    let object = *object;
    let layout = runtime.gc().struct_layout(object).unwrap();
    assert_eq!(
        runtime.gc().struct_get_slot(object, &layout, 0),
        Some(Value::I32(99))
    );
    let hash = call("key_hash", &[key]);
    assert!(matches!(hash, Value::I64(_)));
    assert_eq!(runtime.gc().active_roots(), 1);
    // Six data objects: holder tuple, map, set, Option, payload tuple and identity object.
    // The published program also caches "key" and the encoded Some hash-identity string.
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 8);
    runtime
        .gc()
        .struct_set_slot(object, &layout, 0, Value::I32(101))
        .unwrap();
    assert_eq!(
        runtime.gc().struct_get_slot(object, &layout, 0),
        Some(Value::I32(101))
    );
    assert_eq!(hash, call("key_hash", &[key]));
    assert_eq!(
        call("contains", &[Value::Map(map), Value::Set(set), key]),
        Value::Bool(true)
    );
    let equal = runtime
        .alloc_enum(
            runtime.gc().enum_snapshot(value).unwrap().tag,
            vec![
                runtime
                    .gc()
                    .alloc_tuple(vec![
                        Value::Struct(object),
                        runtime.gc().alloc_string("key".into()).unwrap(),
                    ])
                    .unwrap(),
            ],
        )
        .unwrap();
    let equal_root = runtime.root_value(Value::Enum(equal)).unwrap();
    assert_eq!(
        call("keys_equal", &[key, Value::Enum(equal)]),
        Value::Bool(true)
    );
    assert_eq!(hash, call("key_hash", &[Value::Enum(equal)]));
    assert_eq!(
        call(
            "contains",
            &[Value::Map(map), Value::Set(set), Value::Enum(equal)]
        ),
        Value::Bool(true)
    );
    assert_eq!(runtime.gc().active_roots(), 2);
    // Discard hashing temporaries while retaining the duplicate enum, tuple and string.
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 11);
    drop(equal_root);
    assert_eq!(runtime.gc().active_roots(), 1);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 3);

    // Only the set now retains the composite key and its mutable object.
    runtime.gc().map_clear(map).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 8);
    assert_eq!(runtime.gc().map_snapshot(map), Some(vec![]));
    assert_eq!(runtime.gc().set_snapshot(set), Some(vec![key]));
    assert_eq!(
        runtime.gc().struct_get_slot(object, &layout, 0),
        Some(Value::I32(101))
    );

    // Restore the same key before checking the symmetric map-only retention path.
    assert_eq!(call("restore_map", &[Value::Map(map), key]), Value::Unit);
    runtime.gc().set_clear(set).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 8);
    assert_eq!(
        runtime.gc().map_snapshot(map),
        Some(vec![(key, Value::I32(20))])
    );
    assert_eq!(runtime.gc().set_snapshot(set), Some(vec![]));
    assert_eq!(
        runtime.gc().struct_get_slot(object, &layout, 0),
        Some(Value::I32(101))
    );

    runtime.gc().map_clear(map).unwrap();
    // Only the original enum, tuple and identity object die; both cached strings remain.
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 3);
    assert!(!runtime.gc().validate_value(&key));
    assert!(runtime.gc().struct_layout(object).is_none());
    drop(root);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
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
        .execute(&loaded, ProgramEntry::Main)
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
        .execute(&loaded, ProgramEntry::Main)
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
fn rooted_data_keeps_type_metadata_without_retaining_obsolete_module_state() {
    let captured = Arc::new(Mutex::new(None::<RootedValue>));
    let mut builder = ModuleBuilder::new(
        "fixture::capture",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let capture = builder
        .define_function(FunctionDecl::new("capture"))
        .unwrap();
    builder
        .function(&capture, |function| {
            let value = function.type_parameter("T")?.ty();
            function.parameter("value", value)
        })
        .unwrap();
    let capture_slot = captured.clone();
    builder
        .bind(
            capture,
            NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::unit()), move |cx| {
                let root = cx
                    .heap()
                    .root_value(cx.argument(0)?)
                    .ok_or_else(|| RuntimeError::module_validation("captured data root"))?;
                let mut slot = capture_slot.lock().unwrap();
                assert!(slot.is_none());
                *slot = Some(root);
                Ok(Value::Unit)
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, old) = compile(
        r#"use fixture::capture::capture;
        struct Item { val value: i32 }
        fn main() -> i32 {
            capture(Vec::from([Item { value: 42 }]));
            42
        }"#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&old, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    // Transfer the sole data lease out of the native capture; retain no callable metadata.
    let root = captured.lock().unwrap().take().unwrap();
    assert!(captured.lock().unwrap().is_none());
    assert_eq!(vm.runtime().gc().active_roots(), 1);
    let Value::GcHandle(array) = root.value(vm.runtime().gc()).unwrap() else {
        panic!("typed array")
    };
    let replacement = vm
        .reload_program(
            &old,
            "boundary",
            compile_program("fn main() -> i32 { 99 }", Some(&module)),
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
        vm.execute(&replacement, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(99)
    );
    drop(root);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
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
        .execute(&old, ProgramEntry::Main)
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
        .execute(&current, ProgramEntry::Main)
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
