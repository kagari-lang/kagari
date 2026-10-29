use crate::tests::native_boundary::{compile, publish_unused_version};
use crate::vm::Vm;
use kagari_runtime::{
    error::RuntimeErrorKind,
    gc::GcObjectKind,
    module::{ExecutableModule, entry::ProgramEntry},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        conversion::{FromKagari, KagariType, context::ConversionContext},
        declarations::{FunctionDecl, MethodDecl},
        payload::Opaque,
        storage::{
            NativePayload, NativeStorage,
            access::EditPlan,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
        },
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    range::index_bound_transport,
    reflection,
    resource::HeapCharge,
    value::{Value, ValueCategory},
    value_semantics::identity_equal_transport,
};
use kagari_stdlib::{
    collections::{
        map::{MapStorage, ScriptMap},
        set::{ScriptSet, SetStorage},
        vector::{ScriptVec, VecStorage},
    },
    declarations::StandardDeclarations,
};
use kagari_types::language::Protocol;
use std::ops::Bound;

#[derive(Debug)]
struct Buffer(Vec<i32>);

// SAFETY: This payload contains only scalar integers and has no script edges.
unsafe impl NativePayload for Buffer {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut (),
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        budget.charge(1);
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(self.0.len(), self.0.capacity() * size_of::<i32>())
    }
}

fn decode<T: FromKagari>(vm: &Vm, owner: &ExecutableModule, value: Value) -> T {
    let root = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    ConversionContext::new(vm.runtime(), owner)
        .unwrap()
        .decode(&root)
        .unwrap()
}

fn decode_as<S: KagariType, T: FromKagari>(vm: &Vm, owner: &ExecutableModule, value: Value) -> T {
    let mut cx = ConversionContext::new(vm.runtime(), owner).unwrap();
    let ty = cx.type_for::<S>().unwrap();
    let root = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    let retained = cx.retain(&ty, root).unwrap();
    cx.decode_value(&ty, &retained).unwrap()
}

fn key(cx: &mut NativeContext<'_>, text: &str) -> ScriptValue {
    let expected = cx.conversion().type_for::<String>().unwrap();
    let root = cx.conversion().encode(text.to_owned()).unwrap();
    cx.conversion().retain(&expected, root).unwrap()
}

fn revision<S: KagariType, P: NativePayload>(
    vm: &Vm,
    owner: &ExecutableModule,
    value: Value,
) -> u64 {
    let object: Opaque<P> = decode_as::<S, _>(vm, owner, value);
    // SAFETY: Only the checked revision is read; payload references cannot escape.
    unsafe {
        object
            .read(&vm.context(owner).unwrap(), None, |_, access| {
                Ok(access.revision())
            })
            .unwrap()
    }
}

fn vector_values(vm: &Vm, owner: &ExecutableModule, value: Value) -> Vec<i32> {
    let vector: ScriptVec<i32> = decode(vm, owner, value);
    vector
        .with_slice(&vm.context(owner).unwrap(), |values: &[i32]| {
            Ok(values.to_vec())
        })
        .unwrap()
}

#[test]
fn ordinary_provider_owns_contiguous_scalars_and_identity_keys() {
    let declarations = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::buffers",
        &declarations.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = builder.define_type("Buffer").unwrap();
    declaration.type_parameter("T").unwrap();
    declaration
        .native_storage(NativeStorage::new(|_| Ok(Buffer(Vec::new()))))
        .unwrap();
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
                    // SAFETY: The native argument frame and local working roots protect inputs;
                    // results are decoded, rooted or returned before another callback.
                    unsafe {
                        NativeBinding::new_transport(vec![], buffer.codec(), |context| {
                            context.allocate_result_transport()
                        })
                    },
                )
            })?;
            group.trait_impl(
                declarations.protocol(Protocol::PartialEq).apply([])?,
                |methods| {
                    methods.bind("eq", unsafe {
                        NativeBinding::new_transport(
                            vec![buffer.codec(), buffer.codec()],
                            Codec::Exact(Type::bool()),
                            |context| {
                                identity_equal_transport(
                                    context.heap(),
                                    // SAFETY: The active call retains this input until its copy is published or consumed.
                                    &context.argument_transport(0)?,
                                    // SAFETY: The active call retains this input until its copy is published or consumed.
                                    &context.argument_transport(1)?,
                                )
                                .map(Value::Bool)
                            },
                        )
                    })
                },
            )?;
            group.trait_impl(declarations.eq().apply([])?, |_| Ok(()))?;
            group.trait_impl(declarations.hash().apply([])?, |methods| {
                methods.bind(
                    "hash",
                    // Collisions are intentional; equality still distinguishes buffer identities.
                    unsafe {
                        NativeBinding::new_transport(
                            vec![buffer.codec()],
                            Codec::Exact(Type::i64()),
                            |_| Ok(Value::I64(0)),
                        )
                    },
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
            |cx: &mut NativeContext<'_>,
             (buffer, value): (Opaque<Buffer>, i32)|
             -> NativeResult<()> {
                // SAFETY: Only scalar integers are written; the edit reconciles charge.
                let length = unsafe { buffer.read(cx, None, |payload, _| Ok(payload.0.len()))? };
                unsafe {
                    buffer.edit(
                        cx,
                        None,
                        EditPlan {
                            max_logical_units: length + 1,
                            structural: true,
                        },
                        |payload, access| {
                            payload.0.reserve(1);
                            access.commit_structure();
                            payload.0.push(value);
                            Ok(())
                        },
                    )
                }
            },
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
            |cx: &mut NativeContext<'_>, (buffer,): (Opaque<Buffer>,)| -> NativeResult<i32> {
                // SAFETY: Reversal preserves the scalar-only payload and its charge.
                let length = unsafe { buffer.read(cx, None, |payload, _| Ok(payload.0.len()))? };
                unsafe {
                    buffer.edit(
                        cx,
                        None,
                        EditPlan {
                            max_logical_units: length,
                            structural: false,
                        },
                        |payload, _| {
                            payload.0.reverse();
                            Ok(payload.0.iter().copied().sum())
                        },
                    )
                }
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
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
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
        unsafe {
            index_bound_transport(
                vm.runtime().gc(),
                &valid
                    .return_value
                    .value_transport(vm.runtime().gc())
                    .expect("retained execution result"),
            )
        }
        .unwrap(),
        Bound::Unbounded
    );
    for (name, expected) in [
        ("included", Bound::Included(u64::MAX)),
        ("excluded", Bound::Excluded(1_u64 << 32)),
    ] {
        let report = vm.execute(&loaded, name).unwrap();
        assert_eq!(
            unsafe {
                index_bound_transport(
                    vm.runtime().gc(),
                    &report
                        .return_value
                        .value_transport(vm.runtime().gc())
                        .unwrap(),
                )
            }
            .unwrap(),
            expected
        );
    }
    for name in ["wrong_argument", "wrong_owner"] {
        let invalid = vm.execute(&loaded, name).unwrap();
        assert_eq!(
            unsafe {
                index_bound_transport(
                    vm.runtime().gc(),
                    &invalid
                        .return_value
                        .value_transport(vm.runtime().gc())
                        .expect("retained execution result"),
                )
            }
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
    let result_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: result_root protects the complete tuple throughout these observations.
    let Value::Tuple(values) = unsafe { result_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = unsafe { vm.runtime().gc().tuple_transport(values) }
        .unwrap()
        .to_vec();
    let [
        Value::GcHandle(first),
        Value::GcHandle(second),
        Value::GcHandle(third),
        Value::Struct(fourth),
    ] = values.as_slice()
    else {
        panic!("handles")
    };
    let (first, second, third, fourth) = (*first, *second, *third, *fourth);
    let heap = vm.runtime().gc();
    for (value, expected) in [
        (Value::GcHandle(second), "gc_handle"),
        (Value::GcHandle(third), "gc_handle"),
    ] {
        let Value::Str(id) = unsafe { reflection::type_of_transport(heap, &value) }.unwrap() else {
            panic!("type name")
        };
        assert_eq!(&*unsafe { heap.string_transport(id) }.unwrap(), expected);
    }
    for value in [Value::GcHandle(second), Value::GcHandle(third)] {
        assert_eq!(value.category(), ValueCategory::ScriptOwned);
        assert!(unsafe { value.is_default_heap_payload_transport(heap) });
    }
    assert_ne!(first, second);
    assert_ne!(second, third);
    assert!(
        !unsafe { identity_equal_transport(heap, &Value::GcHandle(third), &Value::Struct(fourth)) }
            .unwrap()
    );
    assert_eq!(
        unsafe { heap.object_kind_transport(first.into()) },
        Some(GcObjectKind::Native)
    );
    assert_eq!(
        unsafe { heap.object_kind_transport(second.into()) },
        Some(GcObjectKind::Native)
    );
    assert_eq!(
        unsafe { heap.object_kind_transport(third.into()) },
        Some(GcObjectKind::Native)
    );
    assert!(unsafe { heap.struct_layout_transport(fourth) }.is_some());
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
    let source_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: The source remains retained until the independent data root is published.
    let value = unsafe { source_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    let root = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    drop(source_root);
    let Value::GcHandle(map) = value else {
        panic!("container")
    };
    vm.runtime().collect_garbage().unwrap();
    let heap = vm.runtime().gc();
    let map: ScriptMap<ScriptValue, i32> =
        decode_as::<ScriptMap<String, i32>, _>(&vm, &loaded, Value::GcHandle(map));
    let mut cx = vm.context(&loaded).unwrap();
    assert_eq!(map.len(&cx).unwrap(), 2);
    assert_eq!(
        sorted_map(map.to_vec(&mut cx).unwrap(), &mut cx),
        vec![("a".into(), Value::I32(1)), ("b".into(), Value::I32(3))]
    );

    let a = key(&mut cx, "a");
    let b = key(&mut cx, "b");
    let c = key(&mut cx, "c");
    let before = heap.stats().current_heap_units;
    map.insert(&mut cx, c.clone(), 4).unwrap();
    assert_eq!(heap.stats().current_heap_units, before + 1);
    assert_eq!(map.get(&mut cx, c.clone()).unwrap(), Some(4));
    map.insert(&mut cx, a.clone(), 9).unwrap();
    assert_eq!(heap.stats().current_heap_units, before + 1);
    assert_eq!(
        sorted_map(map.to_vec(&mut cx).unwrap(), &mut cx),
        vec![
            ("a".into(), Value::I32(9)),
            ("b".into(), Value::I32(3)),
            ("c".into(), Value::I32(4))
        ]
    );
    assert_eq!(map.remove(&mut cx, b.clone()).unwrap(), Some(3));
    assert_eq!(heap.stats().current_heap_units, before);
    map.clear(&mut cx).unwrap();
    assert!(map.to_vec(&mut cx).unwrap().is_empty());
    assert_eq!(heap.stats().current_heap_units, before - 2);
    // Clearing removes the map's ownership of its strings; sweeping reclaims them.
    drop((a, b, c));
    vm.runtime().collect_garbage().unwrap();
    // Empty container (1) plus two one-byte module constants (2 units each).
    assert_eq!(heap.stats().current_heap_units, 5);
    drop(cx);
    drop(map);
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
    let source_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: The source remains retained until the independent data root is published.
    let value = unsafe { source_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    let root = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    drop(source_root);
    let Value::GcHandle(set) = value else {
        panic!("container")
    };
    vm.runtime().collect_garbage().unwrap();
    let heap = vm.runtime().gc();
    let set: ScriptSet<ScriptValue> =
        decode_as::<ScriptSet<String>, _>(&vm, &loaded, Value::GcHandle(set));
    let mut cx = vm.context(&loaded).unwrap();
    assert_eq!(set.len(&cx).unwrap(), 2);
    assert_eq!(
        sorted_set(set.to_vec(&mut cx).unwrap(), &mut cx),
        vec!["a", "b"]
    );

    let a = key(&mut cx, "a");
    let b = key(&mut cx, "b");
    let c = key(&mut cx, "c");
    let before = heap.stats().current_heap_units;
    assert_eq!(set.contains(&mut cx, a.clone()), Ok(true));
    assert_eq!(set.insert(&mut cx, c.clone()), Ok(true));
    assert_eq!(heap.stats().current_heap_units, before + 1);
    assert_eq!(set.insert(&mut cx, a.clone()), Ok(false));
    assert_eq!(heap.stats().current_heap_units, before + 1);
    assert_eq!(set.remove(&mut cx, b.clone()), Ok(true));
    assert_eq!(heap.stats().current_heap_units, before);
    set.clear(&mut cx).unwrap();
    assert!(set.to_vec(&mut cx).unwrap().is_empty());
    assert_eq!(heap.stats().current_heap_units, before - 2);
    drop((a, b, c));
    vm.runtime().collect_garbage().unwrap();
    // Empty container (1) plus two one-byte module constants (2 units each).
    assert_eq!(heap.stats().current_heap_units, 5);
    drop(cx);
    drop(set);
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
        fn main() -> (Record, HashSet<String>, Vec<i32>, String, String) {
            val leaf = Vec::from([1]); val map_key = "leaf"; val set_key = "seen";
            val map: HashMap<String, Vec<i32>> = HashMap::new(); map.insert(map_key, leaf);
            val set: HashSet<String> = HashSet::new(); set.insert(set_key);
            (Record { map: map }, set, leaf, map_key, set_key)
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let result_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: result_root protects the complete tuple throughout these observations.
    let Value::Tuple(values) = unsafe { result_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = unsafe { vm.runtime().gc().tuple_transport(values) }
        .unwrap()
        .to_vec();
    let [
        Value::Struct(record),
        Value::GcHandle(set),
        Value::GcHandle(leaf),
        Value::Str(map_key),
        Value::Str(set_key),
    ] = values.as_slice()
    else {
        panic!("handles")
    };
    let (record, set, leaf, map_key, set_key) = (*record, *set, *leaf, *map_key, *set_key);
    let heap = vm.runtime().gc();
    let schema = unsafe { heap.struct_layout_transport(record) }.unwrap();
    let Value::GcHandle(map) =
        unsafe { heap.struct_get_slot_transport(record, &schema, 0) }.unwrap()
    else {
        panic!("map")
    };
    let root = unsafe {
        heap.root_value_transport(
            heap.alloc_tuple_transport(vec![
                Value::Struct(record),
                Value::GcHandle(set),
                Value::Unit,
            ])
            .unwrap(),
        )
    }
    .unwrap();

    drop(result_root);
    let Value::Tuple(tuple) = unsafe { root.value_transport(heap) }.unwrap() else {
        panic!("root tuple")
    };
    let traced = unsafe { heap.trace_roots_transport() }.unwrap();
    assert_eq!(traced.len(), 7);
    assert_eq!(traced[0], tuple.into());
    assert_eq!(
        traced[2..],
        [map, leaf, map_key, set, set_key].map(Into::into)
    );
    assert_eq!(
        unsafe { heap.object_kind_transport(traced[1]) },
        Some(GcObjectKind::Struct)
    );

    let replacement = unsafe { heap.root_value_transport(Value::GcHandle(leaf)) }.unwrap();
    root.replace(heap, &replacement).unwrap();
    drop(replacement);
    assert_eq!(
        unsafe { heap.trace_roots_transport() }.unwrap(),
        vec![leaf.into()]
    );

    assert_eq!(
        unsafe { root.value_transport(heap) }.unwrap(),
        Value::GcHandle(leaf)
    );
    drop(root);
    assert!(unsafe { heap.trace_roots_transport() }.unwrap().is_empty());
}

#[test]
fn root_scanning_handles_cycles_without_duplicate_identity() {
    let (vm, loaded) = compile(
        r#"
        struct Cycle { val array: Vec<Cycle> }
        fn main() -> (Vec<Cycle>, Cycle) { val array: Vec<Cycle> = Vec::new(); val record = Cycle { array: array }; array.push(record); (array, record) }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let result_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: result_root protects the complete tuple throughout these observations.
    let Value::Tuple(values) = unsafe { result_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result")
    else {
        panic!("array")
    };
    let heap = vm.runtime().gc();
    let values = unsafe { heap.tuple_transport(values) }.unwrap().to_vec();
    let [Value::GcHandle(array), Value::Struct(record)] = values.as_slice() else {
        panic!("cycle")
    };
    let (array, record) = (*array, *record);
    let _root = unsafe { heap.root_value_transport(Value::GcHandle(array)) }.unwrap();
    drop(result_root);

    vm.runtime().collect_garbage().unwrap();
    let traced = unsafe { heap.trace_roots_transport() }.unwrap();
    assert_eq!(traced.len(), 2);
    assert_eq!(traced[0], array.into());
    assert_ne!(traced[0], traced[1]);
    assert_eq!(
        unsafe { heap.object_kind_transport(traced[1]) },
        Some(GcObjectKind::Struct)
    );
    let shape = unsafe { heap.struct_layout_transport(record) }.unwrap();
    assert_eq!(
        unsafe { heap.struct_get_slot_transport(record, &shape, 0) },
        Some(Value::GcHandle(array))
    );
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
    let result_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: result_root protects the complete tuple throughout these observations.
    let Value::Tuple(values) = unsafe { result_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = unsafe { vm.runtime().gc().tuple_transport(values) }
        .unwrap()
        .to_vec();
    let [
        Value::GcHandle(array),
        Value::GcHandle(map),
        Value::GcHandle(set),
    ] = values.as_slice()
    else {
        panic!("handles")
    };
    let (array, map, set) = (*array, *map, *set);
    let heap = vm.runtime().gc();
    let vector: ScriptVec<i32> = decode(&vm, &loaded, Value::GcHandle(array));
    let map: ScriptMap<i32, i32> = decode(&vm, &loaded, Value::GcHandle(map));
    let set: ScriptSet<i32> = decode(&vm, &loaded, Value::GcHandle(set));
    let mut cx = vm.context(&loaded).unwrap();
    assert_eq!(vector.pop(&mut cx).unwrap(), None);
    assert_eq!(vector.remove(&mut cx, 0).unwrap(), None);
    assert_eq!(map.remove(&mut cx, 1).unwrap(), None);
    assert!(!set.remove(&mut cx, 1).unwrap());
    let mut conversion = ConversionContext::new(vm.runtime(), &loaded).unwrap();
    let wrong_map = conversion.type_for::<ScriptMap<f64, i32>>().unwrap();
    let wrong_set = conversion.type_for::<ScriptSet<f64>>().unwrap();
    assert!(
        // SAFETY: The execution result retains this live container; only its type is wrong.
        unsafe { conversion.decode_transport::<ScriptMap<f64, i32>>(&wrong_map, &values[1]) }
            .is_err()
    );
    assert!(
        // SAFETY: The execution result retains this live container; only its type is wrong.
        unsafe { conversion.decode_transport::<ScriptSet<f64>>(&wrong_set, &values[2]) }.is_err()
    );
    let before = heap.stats().current_heap_units;
    vector.clear(&mut cx).unwrap();
    map.clear(&mut cx).unwrap();
    set.clear(&mut cx).unwrap();
    assert_eq!(heap.stats().current_heap_units, before);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        vector.pop(&mut cx).unwrap(),
        None,
        "owning handles preserve empty objects"
    );
    vm.runtime().close().unwrap();
    assert!(vector.pop(&mut cx).is_err());
    assert!(vector.remove(&mut cx, 0).is_err());
    assert!(vector.clear(&mut cx).is_err());
    assert!(map.remove(&mut cx, 1).is_err());
    assert!(map.clear(&mut cx).is_err());
    assert!(set.remove(&mut cx, 1).is_err());
    assert!(set.clear(&mut cx).is_err());
}

fn sorted_map(
    entries: Vec<(ScriptValue, i32)>,
    cx: &mut NativeContext<'_>,
) -> Vec<(String, Value)> {
    let mut entries: Vec<_> = entries
        .into_iter()
        .map(|(key, value)| (key.decode::<String>(cx).unwrap(), Value::I32(value)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn sorted_set(entries: Vec<ScriptValue>, cx: &mut NativeContext<'_>) -> Vec<String> {
    let mut entries: Vec<_> = entries
        .into_iter()
        .map(|key| key.decode::<String>(cx).unwrap())
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
    let result_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: result_root protects the complete tuple throughout these observations.
    let Value::Tuple(values) = unsafe { result_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = unsafe { vm.runtime().gc().tuple_transport(values) }
        .unwrap()
        .to_vec();
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
        vector_values(&vm, &loaded, Value::GcHandle(*array)),
        vec![1, 3]
    );
    let removed = unsafe { heap.enum_snapshot_transport(*removed) }.unwrap();
    assert_eq!(removed.tag.variant_name(), "Some");
    assert_eq!(removed.fields, vec![Value::I32(2)]);
    let missing = unsafe { heap.enum_snapshot_transport(*missing) }.unwrap();
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
    let result_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: result_root protects the complete tuple throughout these observations.
    let Value::Tuple(values) = unsafe { result_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = unsafe { vm.runtime().gc().tuple_transport(values) }
        .unwrap()
        .to_vec();
    let [
        Value::GcHandle(map),
        Value::Enum(removed),
        Value::Enum(missing),
        Value::Bool(contains),
    ] = values.as_slice()
    else {
        panic!("results")
    };
    let heap = vm.runtime().gc();
    assert!(*contains);
    let removed = unsafe { heap.enum_snapshot_transport(*removed) }.unwrap();
    assert_eq!(removed.tag.variant_name(), "Some");
    assert_eq!(removed.fields, vec![Value::I32(100)]);
    assert_eq!(
        unsafe { heap.enum_snapshot_transport(*missing) }
            .unwrap()
            .tag
            .variant_name(),
        "None"
    );
    let mut cx = vm.context(&loaded).unwrap();
    let typed: ScriptMap<ScriptValue, i32> =
        decode_as::<ScriptMap<String, i32>, _>(&vm, &loaded, Value::GcHandle(*map));
    assert_eq!(
        sorted_map(typed.to_vec(&mut cx).unwrap(), &mut cx),
        vec![("mp".into(), Value::I32(20))]
    );
    let mut conversion = ConversionContext::new(vm.runtime(), &loaded).unwrap();
    let wrong = conversion.type_for::<ScriptMap<String, i64>>().unwrap();
    let before = heap.stats();
    assert!(
        // SAFETY: The execution result retains this live map; only its value type is wrong.
        unsafe {
            conversion.decode_transport::<ScriptMap<String, i64>>(&wrong, &Value::GcHandle(*map))
        }
        .is_err()
    );
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
    let result_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: result_root protects the complete tuple throughout these observations.
    let Value::Tuple(values) = unsafe { result_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result")
    else {
        panic!("tuple")
    };
    let values = unsafe { vm.runtime().gc().tuple_transport(values) }
        .unwrap()
        .to_vec();
    let [
        Value::GcHandle(array),
        Value::GcHandle(map),
        Value::GcHandle(set),
    ] = values.as_slice()
    else {
        panic!("handles")
    };
    let (array, map, set) = (*array, *map, *set);
    let sources = [
        Value::GcHandle(array),
        Value::GcHandle(map),
        Value::GcHandle(set),
    ];
    let current = || {
        [
            revision::<ScriptVec<i32>, VecStorage>(&vm, &loaded, sources[0]),
            revision::<ScriptMap<i32, i32>, MapStorage>(&vm, &loaded, sources[1]),
            revision::<ScriptSet<i32>, SetStorage>(&vm, &loaded, sources[2]),
        ]
    };
    let revisions = current();
    let vector: ScriptVec<i32> = decode(&vm, &loaded, sources[0]);
    let map: ScriptMap<i32, i32> = decode(&vm, &loaded, sources[1]);
    let set: ScriptSet<i32> = decode(&vm, &loaded, sources[2]);
    let mut cx = vm.context(&loaded).unwrap();
    vector.set(&mut cx, 0, 9).unwrap();
    map.insert(&mut cx, 1, 9).unwrap();
    assert!(!set.insert(&mut cx, 1).unwrap());
    assert_eq!(current(), revisions);
    vector.push(&mut cx, 2).unwrap();
    map.insert(&mut cx, 2, 3).unwrap();
    assert!(set.insert(&mut cx, 2).unwrap());
    assert!(
        current()
            .into_iter()
            .zip(revisions)
            .all(|(now, old)| now > old)
    );
    assert_eq!(vector_values(&vm, &loaded, sources[0]), vec![9, 2]);
    assert_eq!(map.len(&cx).unwrap(), 2);
    assert_eq!(set.len(&cx).unwrap(), 2);
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
    let source_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: The source remains retained until the independent data root is published.
    let value = unsafe { source_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    let root = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    drop(source_root);
    let Value::Tuple(values) = value else {
        panic!("tuple")
    };
    let values = unsafe { vm.runtime().gc().tuple_transport(values) }
        .unwrap()
        .to_vec();
    let [Value::GcHandle(map), Value::GcHandle(set)] = values.as_slice() else {
        panic!("handles")
    };
    let runtime = vm.runtime();
    let cx = vm.context(&loaded).unwrap();
    let map: ScriptMap<String, i32> = decode(&vm, &loaded, Value::GcHandle(*map));
    let set: ScriptSet<String> = decode(&vm, &loaded, Value::GcHandle(*set));
    assert_eq!(map.len(&cx).unwrap(), 2);
    assert_eq!(set.len(&cx).unwrap(), 2);
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
