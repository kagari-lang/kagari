mod payload_failures;
mod program_cycles;
use super::{compile, compile_program};
use crate::vm::owned::DriveResult;
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootedValue,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        conversion::{IntoKagari, context::ConversionContext},
        declarations::FunctionDecl,
        types::Type,
        value_handle::ScriptValue,
    },
    value::Value,
    value_semantics::format_value,
};
use kagari_stdlib::{
    collections::{map::ScriptMap, set::ScriptSet, vector::ScriptVec},
    declarations::StandardDeclarations,
};
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
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
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
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
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
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let result: ScriptValue = vm.execute_typed(&loaded, ProgramEntry::Main, ()).unwrap();
    let mut conversion = ConversionContext::new(vm.runtime(), &loaded).unwrap();
    let map_type = conversion.parameter(result.type_argument(), 0).unwrap();
    let set_type = conversion.parameter(result.type_argument(), 1).unwrap();
    let result_type = result.type_argument().clone();
    let Value::Tuple(values) = result.into_kagari(&mut conversion, &result_type).unwrap() else {
        panic!("containers")
    };
    drop(conversion);
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [
        Value::GcHandle(map),
        Value::GcHandle(set),
        Value::Bool(found),
    ] = values.as_slice()
    else {
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
                .alloc_tuple(vec![Value::GcHandle(map), Value::GcHandle(set)])
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
    let map_handle = || -> ScriptMap<ScriptValue, i32> {
        ConversionContext::new(runtime, &loaded)
            .unwrap()
            .decode_value(&map_type, &Value::GcHandle(map))
            .unwrap()
    };
    let set_handle = || -> ScriptSet<ScriptValue> {
        ConversionContext::new(runtime, &loaded)
            .unwrap()
            .decode_value(&set_type, &Value::GcHandle(set))
            .unwrap()
    };
    let map_entries = || {
        let mut result = Vec::new();
        map_handle()
            .for_each(&mut vm.context(&loaded).unwrap(), |cx, key, value| {
                let ty = key.type_argument().clone();
                result.push((key.into_kagari(cx.conversion(), &ty)?, Value::I32(value)));
                Ok(())
            })
            .unwrap();
        result
    };
    let set_entries = || {
        let mut result = Vec::new();
        set_handle()
            .for_each(&mut vm.context(&loaded).unwrap(), |cx, key| {
                let ty = key.type_argument().clone();
                result.push(key.into_kagari(cx.conversion(), &ty)?);
                Ok(())
            })
            .unwrap();
        result
    };
    let key = map_entries()[0].0;
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
        call(
            "contains",
            &[Value::GcHandle(map), Value::GcHandle(set), key]
        ),
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
            &[
                Value::GcHandle(map),
                Value::GcHandle(set),
                Value::Enum(equal)
            ]
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
    map_handle()
        .clear(&mut vm.context(&loaded).unwrap())
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 8);
    assert_eq!(map_entries(), vec![]);
    assert_eq!(set_entries(), vec![key]);
    assert_eq!(
        runtime.gc().struct_get_slot(object, &layout, 0),
        Some(Value::I32(101))
    );

    // Restore the same key before checking the symmetric map-only retention path.
    assert_eq!(
        call("restore_map", &[Value::GcHandle(map), key]),
        Value::Unit
    );
    set_handle()
        .clear(&mut vm.context(&loaded).unwrap())
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 8);
    assert_eq!(map_entries(), vec![(key, Value::I32(20))]);
    assert_eq!(set_entries(), vec![]);
    assert_eq!(
        runtime.gc().struct_get_slot(object, &layout, 0),
        Some(Value::I32(101))
    );

    map_handle()
        .clear(&mut vm.context(&loaded).unwrap())
        .unwrap();
    // Only the original enum, tuple and identity object die; both cached strings remain.
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 3);
    assert!(!runtime.gc().validate_value(&key));
    assert!(runtime.gc().struct_layout(object).is_none());
    drop(root);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
    let candidate = runtime
        .stage_reload_verified_program(&_loaded_program, None, loaded.verified_program().clone())
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
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let (map, set): (ScriptMap<ScriptValue, i32>, ScriptSet<ScriptValue>) =
        vm.execute_typed(&loaded, ProgramEntry::Main, ()).unwrap();
    let (foreign_vm, foreign_program) = compile("fn main() -> Vec<i32> { Vec::from([]) }", None);
    let foreign_owner = foreign_vm
        .runtime()
        .select_program(&foreign_program)
        .unwrap();
    let foreign: ScriptValue = foreign_vm
        .execute_typed(&foreign_owner, ProgramEntry::Main, ())
        .unwrap();
    let mut cx = vm.context(&loaded).unwrap();
    assert!(map.insert(&mut cx, foreign.clone(), 0).is_err());
    assert!(set.insert(&mut cx, foreign).is_err());
    let f64_type = cx.conversion().type_for::<f64>().unwrap();
    let wrong: ScriptValue = cx
        .conversion()
        .decode_value(&f64_type, &Value::F64(1.0))
        .unwrap();
    assert!(map.insert(&mut cx, wrong, 0).is_err());
    let i32_type = cx.conversion().type_for::<i32>().unwrap();
    let key: ScriptValue = cx
        .conversion()
        .decode_value(&i32_type, &Value::I32(1))
        .unwrap();
    assert_eq!(map.len(&cx).unwrap(), 1);
    assert_eq!(map.get(&mut cx, key.clone()).unwrap(), Some(42));
    assert_eq!(set.len(&cx).unwrap(), 1);
    assert!(set.contains(&mut cx, key).unwrap());
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
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let result: ScriptValue = vm.execute_typed(&loaded, ProgramEntry::Main, ()).unwrap();
    let mut cx = vm.context(&loaded).unwrap();
    let vector: ScriptVec<ScriptValue> = result.decode(&mut cx).unwrap();
    let ty = result.type_argument().clone();
    let value = result.into_kagari(cx.conversion(), &ty).unwrap();
    let runtime = vm.runtime();
    let preview = format_value(runtime.gc(), &value, true).unwrap();
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
    assert_eq!(vector.len(&cx).unwrap(), 1);
}

#[derive(Clone, Copy, Debug)]
enum DataCollection {
    Vector,
    ScalarMap,
    ReferenceMap,
    ScalarSet,
    ReferenceSet,
}

#[derive(Clone)]
enum DataAdapter {
    Vector(ScriptVec<ScriptValue>),
    Map(ScriptMap<i32, ScriptValue>),
    Set(ScriptSet<ScriptValue>),
}

impl DataAdapter {
    fn decode(
        kind: DataCollection,
        cx: &mut ConversionContext<'_>,
        ty: &TypeArgument,
        value: &Value,
    ) -> Self {
        match kind {
            DataCollection::Vector => Self::Vector(cx.decode_value(ty, value).unwrap()),
            DataCollection::ScalarMap | DataCollection::ReferenceMap => {
                Self::Map(cx.decode_value(ty, value).unwrap())
            }
            DataCollection::ScalarSet | DataCollection::ReferenceSet => {
                Self::Set(cx.decode_value(ty, value).unwrap())
            }
        }
    }
}

#[test]
fn rooted_data_keeps_type_metadata_without_retaining_obsolete_module_state() {
    for kind in [
        DataCollection::Vector,
        DataCollection::ScalarMap,
        DataCollection::ReferenceMap,
        DataCollection::ScalarSet,
        DataCollection::ReferenceSet,
    ] {
        for retention in 0..3 {
            check_data_metadata_retention(kind, retention);
        }
    }
}

fn check_data_metadata_retention(kind: DataCollection, retention: u8) {
    let captured = Arc::new(Mutex::new(None::<(RootedValue, TypeArgument)>));
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
                *slot = Some((root, cx.argument_type_argument(0)?));
                Ok(Value::Unit)
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let recipe = match kind {
        DataCollection::Vector => "capture(Vec::from([Item { value: 42 }]));",
        DataCollection::ScalarMap => {
            "val map: HashMap<i32, i32> = HashMap::new(); map.insert(1, 42); capture(map);"
        }
        DataCollection::ReferenceMap => {
            "val map: HashMap<i32, Item> = HashMap::new(); map.insert(1, Item { value: 42 }); capture(map);"
        }
        DataCollection::ScalarSet => {
            "val set: HashSet<i32> = HashSet::new(); set.insert(42); capture(set);"
        }
        DataCollection::ReferenceSet => {
            r#"val set: HashSet<String> = HashSet::new(); set.insert("42"); capture(set);"#
        }
    };
    let source = format!(
        "use fixture::capture::capture; use std::collections::{{HashMap, HashSet}}; struct Item {{ val value: i32 }} pub fn retained_recipe() {{ {recipe} }} pub fn main() -> i32 {{ retained_recipe(); 42 }}"
    );
    let (vm, program) = compile(&source, Some(&module));
    let old = vm.runtime().select_program(&program).unwrap();
    assert_eq!(vm.execute_typed::<(), i32>(&old, "main", ()).unwrap(), 42);
    let (root, collection_type) = captured.lock().unwrap().take().unwrap();
    let mut root = Some(root);
    let raw = root.as_ref().unwrap().value(vm.runtime().gc()).unwrap();
    let script_value: Option<ScriptValue> = (retention == 1).then(|| {
        let value = ConversionContext::new(vm.runtime(), &old)
            .unwrap()
            .decode_value(&collection_type, &raw)
            .unwrap();
        drop(root.take());
        value
    });
    let adapter = (retention == 2).then(|| {
        let value = DataAdapter::decode(
            kind,
            &mut ConversionContext::new(vm.runtime(), &old).unwrap(),
            &collection_type,
            &raw,
        );
        drop(root.take());
        value
    });
    assert_eq!(vm.runtime().gc().active_roots(), 1);
    vm.reload_program(
        &program,
        Some("boundary".into()),
        compile_program(
            &source.replace("retained_recipe(); 42", "99"),
            Some(&module),
        ),
    )
    .unwrap();
    let replacement = vm.runtime().select_program(&program).unwrap();
    let expected: HashSet<_> = old.members().map(|member| member.key()).collect();
    let reclaimed: HashSet<_> = vm
        .runtime()
        .collect_garbage()
        .unwrap()
        .reclaimed_modules
        .into_iter()
        .collect();
    // Hash containers have actual selected Hash/Eq graph edges, including scalar
    // native selections. The raw-root variant establishes that independent owner;
    // Vec has no executable edge and must reclaim the entire obsolete program.
    if matches!(kind, DataCollection::Vector) {
        assert_eq!(reclaimed, expected, "{kind:?}, retention={retention}");
        assert!(vm.runtime().validate_executable_module(&old).is_err());
    } else {
        assert!(reclaimed.is_empty(), "{kind:?}, retention={retention}");
        vm.runtime().validate_executable_module(&old).unwrap();
    }
    let objects = match kind {
        DataCollection::ScalarMap | DataCollection::ScalarSet => 1,
        _ => 2,
    };
    assert_eq!(
        vm.runtime().collect_garbage().unwrap().live_objects,
        objects
    );
    {
        let mut cx = vm.context(&replacement).unwrap();
        let adapter = adapter.clone().unwrap_or_else(|| {
            let value = script_value.as_ref().map_or_else(
                || root.as_ref().unwrap().value(vm.runtime().gc()).unwrap(),
                |value| value.value(vm.runtime()).unwrap(),
            );
            DataAdapter::decode(kind, cx.conversion(), &collection_type, &value)
        });
        let bool_type = cx.conversion().type_for::<bool>().unwrap();
        let wrong: ScriptValue = cx
            .conversion()
            .decode_value(&bool_type, &Value::Bool(true))
            .unwrap();
        let element_index = usize::from(matches!(
            kind,
            DataCollection::ScalarMap | DataCollection::ReferenceMap
        ));
        let element = cx
            .conversion()
            .parameter(&collection_type, element_index)
            .unwrap();
        let item = match &adapter {
            DataAdapter::Vector(vector) => vector.get(&mut cx, 0).unwrap().unwrap(),
            DataAdapter::Map(map) => map.get(&mut cx, 1).unwrap().unwrap(),
            DataAdapter::Set(set) => {
                let mut item = None;
                set.for_each(&mut cx, |_, value| {
                    item = Some(value);
                    Ok(())
                })
                .unwrap();
                item.unwrap()
            }
        };
        assert_eq!(item.type_argument().ty(), element.ty());
        let raw_item = item.value(vm.runtime()).unwrap();
        if let Value::Struct(id) = raw_item {
            let (_, fields) = vm.runtime().gc().struct_snapshot(id).unwrap();
            assert_eq!(fields[0].value, Value::I32(42));
        } else if matches!(kind, DataCollection::ReferenceSet) {
            assert_eq!(
                format_value(vm.runtime().gc(), &raw_item, false).unwrap(),
                "42"
            );
        } else {
            assert_eq!(raw_item, Value::I32(42));
        }
        match adapter {
            DataAdapter::Vector(vector) => {
                vector.push(&mut cx, item).unwrap();
                assert!(vector.push(&mut cx, wrong).is_err());
                assert_eq!(vector.len(&cx).unwrap(), 2);
            }
            DataAdapter::Map(map) => {
                map.insert(&mut cx, 2, item).unwrap();
                assert!(map.insert(&mut cx, 3, wrong).is_err());
                assert_eq!(map.len(&cx).unwrap(), 2);
            }
            DataAdapter::Set(set) => {
                assert!(set.contains(&mut cx, item.clone()).unwrap());
                assert!(!set.insert(&mut cx, item).unwrap());
                assert!(set.insert(&mut cx, wrong).is_err());
                assert_eq!(set.len(&cx).unwrap(), 1);
            }
        }
    }
    assert_eq!(
        vm.execute_typed::<(), i32>(&replacement, "main", ())
            .unwrap(),
        99
    );
    drop((root, script_value, adapter));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_objects, objects);
    if !matches!(kind, DataCollection::Vector) {
        assert_eq!(
            collected
                .reclaimed_modules
                .into_iter()
                .collect::<HashSet<_>>(),
            expected
        );
    }
}

#[test]
fn recursive_element_contracts_reject_foreign_nested_layouts_after_reload() {
    let source = r#"
        struct Inner { val value: i32 }
        struct Node { var next: Option<Node>, val value: Inner }
        fn main() -> Vec<Node> {
            val node = Node { next: None, value: Inner { value: 42 } };
            node.next = Some(node);
            Vec::from([node])
        }
    "#;
    let (mut vm, old) = compile(source, None);
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    let result: ScriptValue = vm.execute_typed(&old, ProgramEntry::Main, ()).unwrap();
    let old_type = result.type_argument().clone();
    let old_value = result
        .into_kagari(
            &mut ConversionContext::new(vm.runtime(), &old).unwrap(),
            &old_type,
        )
        .unwrap();
    let old_root = vm.runtime().root_value(old_value).unwrap();
    let replacement = source
        .replace("val value: i32", "val value: i32, val extra: bool")
        .replace("value: 42 }", "value: 42, extra: true }");
    assert!(
        vm.reload_program(&_old_program, None, compile_program(&replacement, None))
            .is_err()
    );
    vm.reload_program(
        &_old_program,
        None,
        compile_program(&source.replace("42", "43"), None),
    )
    .unwrap();
    let independent = vm
        .runtime_mut()
        .load_program("foreign-nested-layout", compile_program(&replacement, None))
        .unwrap();
    let current = vm.runtime().select_program(&independent).unwrap();
    let result: ScriptValue = vm.execute_typed(&current, ProgramEntry::Main, ()).unwrap();
    let new_type = result.type_argument().clone();
    let new_value = result
        .into_kagari(
            &mut ConversionContext::new(vm.runtime(), &current).unwrap(),
            &new_type,
        )
        .unwrap();
    let new_root = vm.runtime().root_value(new_value).unwrap();
    let reclaimed = vm.runtime().collect_garbage().unwrap().reclaimed_modules;
    assert!(
        old.members()
            .all(|member| reclaimed.contains(&member.key()))
    );
    {
        let replacement = vm.runtime().select_program(&_old_program).unwrap();
        let mut old_cx = vm.context(&replacement).unwrap();
        let mut new_cx = vm.context(&current).unwrap();
        let old_vector: ScriptVec<ScriptValue> = old_cx
            .conversion()
            .decode_value(&old_type, &old_value)
            .unwrap();
        let new_vector: ScriptVec<ScriptValue> = new_cx
            .conversion()
            .decode_value(&new_type, &new_value)
            .unwrap();
        let old_node = old_vector.get(&mut old_cx, 0).unwrap().unwrap();
        let new_node = new_vector.get(&mut new_cx, 0).unwrap().unwrap();
        assert!(old_vector.push(&mut old_cx, new_node.clone()).is_err());
        assert!(new_vector.push(&mut new_cx, old_node.clone()).is_err());
        assert_eq!(old_vector.len(&old_cx).unwrap(), 1);
        assert_eq!(new_vector.len(&new_cx).unwrap(), 1);
        old_vector.push(&mut old_cx, old_node).unwrap();
        new_vector.push(&mut new_cx, new_node).unwrap();
        assert_eq!(old_vector.len(&old_cx).unwrap(), 2);
        assert_eq!(new_vector.len(&new_cx).unwrap(), 2);
    }
    drop((old_root, new_root));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}
