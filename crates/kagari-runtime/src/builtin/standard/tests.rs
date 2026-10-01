use super::*;
use kagari_abi::standard::RuntimePrimitive;

use crate::gc::{GcHeap, GcHeapConfig};

#[test]
fn join_validates_native_arguments_and_leaves_the_array_unchanged() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let handle = heap
        .alloc_array(vec![Value::Str("é".into()), Value::Str("😀".into())])
        .unwrap();
    let before = heap.array_snapshot(handle).unwrap();
    assert_eq!(
        array_join(&heap, &[Value::Array(handle), Value::Str("/".into())]).unwrap(),
        Value::Str("é/😀".into())
    );
    assert_eq!(heap.array_snapshot(handle).unwrap(), before);
    assert!(array_join(&heap, &[Value::I32(1), Value::Str("".into())]).is_err());
    let invalid = heap.alloc_array(vec![Value::I32(1)]).unwrap();
    assert!(array_join(&heap, &[Value::Array(invalid), Value::Str("".into())]).is_err());
    assert_eq!(heap.array_snapshot(invalid).unwrap(), vec![Value::I32(1)]);
    assert!(
        array_join(
            &GcHeap::new(
                GcHeapConfig::default(),
                std::rc::Rc::new(crate::resource::ResourceState::default())
            ),
            &[Value::Array(handle), Value::Str("".into())]
        )
        .is_err()
    );
}

fn call(gc: &GcHeap, intrinsic: RuntimePrimitive, args: &[Value]) -> Result<Value, BuiltinError> {
    invoke(gc, intrinsic, args)
}

fn option_variant(gc: &GcHeap, value: &Value) -> (String, Vec<Value>) {
    let Value::Enum(handle) = value else {
        panic!("expected enum value");
    };
    let snapshot = gc.enum_snapshot(*handle).unwrap();
    assert_eq!(snapshot.tag.type_name(), "Option");
    (snapshot.tag.variant_name().to_owned(), snapshot.fields)
}

#[test]
fn builtin_standard_array_helpers_mutate_and_return_options() {
    let gc = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let array = Value::Array(gc.alloc_array(vec![Value::I32(1)]).unwrap());

    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::ArrayLen,
            std::slice::from_ref(&array)
        )
        .unwrap(),
        Value::U64(1)
    );
    call(
        &gc,
        RuntimePrimitive::ArrayPush,
        &[array.clone(), Value::I32(3)],
    )
    .unwrap();
    call(
        &gc,
        RuntimePrimitive::ArrayInsert,
        &[array.clone(), Value::U64(1), Value::I32(2)],
    )
    .unwrap();
    assert_eq!(
        gc.array_snapshot(match array {
            Value::Array(handle) => handle,
            _ => unreachable!(),
        })
        .unwrap(),
        vec![Value::I32(1), Value::I32(2), Value::I32(3)]
    );

    let removed = call(
        &gc,
        RuntimePrimitive::ArrayRemove,
        &[array.clone(), Value::I32(1)],
    )
    .unwrap();
    assert_eq!(
        option_variant(&gc, &removed),
        ("Some".to_owned(), vec![Value::I32(2)])
    );
    let missing = call(&gc, RuntimePrimitive::ArrayGet, &[array, Value::I32(99)]).unwrap();
    assert_eq!(
        option_variant(&gc, &missing),
        ("None".to_owned(), Vec::new())
    );
}

#[test]
fn builtin_standard_map_helpers_preserve_order_and_return_options() {
    let gc = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let map = call(&gc, RuntimePrimitive::LinkedHashMapNew, &[]).unwrap();
    call(
        &gc,
        RuntimePrimitive::MapInsert,
        &[map.clone(), Value::Str("hp".to_owned()), Value::I32(100)],
    )
    .unwrap();
    call(
        &gc,
        RuntimePrimitive::MapInsert,
        &[map.clone(), Value::Str("mp".to_owned()), Value::I32(40)],
    )
    .unwrap();

    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::MapContainsKey,
            &[map.clone(), Value::Str("hp".to_owned())]
        )
        .unwrap(),
        Value::Bool(true)
    );
    let keys = call(
        &gc,
        RuntimePrimitive::MapKeysStorage,
        std::slice::from_ref(&map),
    )
    .unwrap();
    let Value::Array(keys) = keys else {
        panic!("expected key array");
    };
    assert_eq!(
        gc.array_snapshot(keys).unwrap(),
        vec![Value::Str("hp".to_owned()), Value::Str("mp".to_owned())]
    );
    let removed = call(
        &gc,
        RuntimePrimitive::MapRemove,
        &[map.clone(), Value::Str("hp".to_owned())],
    )
    .unwrap();
    assert_eq!(
        option_variant(&gc, &removed),
        ("Some".to_owned(), vec![Value::I32(100)])
    );
    let missing = call(
        &gc,
        RuntimePrimitive::MapGet,
        &[map, Value::Str("hp".to_owned())],
    )
    .unwrap();
    assert_eq!(
        option_variant(&gc, &missing),
        ("None".to_owned(), Vec::new())
    );
}

#[test]
fn builtin_standard_string_helpers_validate_utf8_boundaries() {
    let gc = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::StringLenBytes,
            &[Value::Str("éx".to_owned())]
        )
        .unwrap(),
        Value::U64(3)
    );
    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::StringLenChars,
            &[Value::Str("éx".to_owned())]
        )
        .unwrap(),
        Value::U64(2)
    );
    let good = call(
        &gc,
        RuntimePrimitive::StringSlice,
        &[Value::Str("éx".to_owned()), Value::U64(0), Value::U64(2)],
    )
    .unwrap();
    assert_eq!(
        option_variant(&gc, &good),
        ("Some".to_owned(), vec![Value::Str("é".to_owned())])
    );
    let bad = call(
        &gc,
        RuntimePrimitive::StringSlice,
        &[Value::Str("éx".to_owned()), Value::U64(1), Value::U64(2)],
    )
    .unwrap();
    assert_eq!(option_variant(&gc, &bad), ("None".to_owned(), Vec::new()));
}

#[test]
fn builtin_standard_option_result_helpers_use_standard_enum_values() {
    let gc = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let some = option_some(&gc, Value::I32(10)).unwrap();
    let none = option_none(&gc).unwrap();
    let ok = enum_value(&gc, EnumTag::ResultOk, vec![Value::I32(7)]).unwrap();
    let err = enum_value(&gc, EnumTag::ResultErr, vec![Value::Str("no".to_owned())]).unwrap();

    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::OptionUnwrapOr,
            &[some.clone(), Value::I32(0)]
        )
        .unwrap(),
        Value::I32(10)
    );
    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::OptionUnwrapOr,
            &[none, Value::I32(0)]
        )
        .unwrap(),
        Value::I32(0)
    );
    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::ResultUnwrapOr,
            &[ok.clone(), Value::I32(0)]
        )
        .unwrap(),
        Value::I32(7)
    );
    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::ResultIsErr,
            std::slice::from_ref(&err)
        )
        .unwrap(),
        Value::Bool(true)
    );

    // Callback semantics are covered by the source/decoded native family matrix.
    // Unvalidated physical calls cannot select the resumable implementation.
    assert!(call(&gc, RuntimePrimitive::OptionMap, &[some, Value::Unit]).is_err());
    assert!(call(&gc, RuntimePrimitive::ResultAndThen, &[ok, Value::Unit]).is_err());
}

#[test]
fn builtin_standard_math_and_debug_helpers_are_deterministic() {
    let gc = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::MathMin,
            &[Value::I64(8), Value::I64(3)]
        )
        .unwrap(),
        Value::I64(3)
    );
    assert_eq!(
        call(
            &gc,
            RuntimePrimitive::MathClamp,
            &[Value::I32(12), Value::I32(0), Value::I32(10)]
        )
        .unwrap(),
        Value::I32(10)
    );
    assert_eq!(
        call(&gc, RuntimePrimitive::MathSqrt, &[Value::F64(9.0)]).unwrap(),
        Value::F64(3.0)
    );
    assert!(call(&gc, RuntimePrimitive::MathSqrt, &[Value::F64(-1.0)]).is_err());
    assert!(
        call(
            &gc,
            RuntimePrimitive::AssertEq,
            &[Value::I32(1), Value::I32(1), Value::Str("same".into())]
        )
        .is_err()
    );
    assert!(
        call(
            &gc,
            RuntimePrimitive::DebugPanic,
            &[Value::Str("boom".to_owned())]
        )
        .is_err()
    );
}
#[test]
fn collection_iteration_rejects_structural_alias_writes_before_allocation() {
    let gc = GcHeap::new(Default::default(), Default::default());
    let array = Value::Array(gc.alloc_array(vec![Value::I32(1)]).unwrap());
    let map = Value::Map(gc.alloc_map(vec![(Value::I32(1), Value::I32(2))]).unwrap());
    let set = Value::Set(gc.alloc_set(vec![Value::I32(1)]).unwrap());
    let operations = [
        (
            RuntimePrimitive::ArrayPush,
            vec![array.clone(), Value::I32(2)],
        ),
        (RuntimePrimitive::ArrayPop, vec![array.clone()]),
        (
            RuntimePrimitive::ArrayInsert,
            vec![array.clone(), Value::I32(0), Value::I32(2)],
        ),
        (
            RuntimePrimitive::ArrayRemove,
            vec![array.clone(), Value::I32(0)],
        ),
        (RuntimePrimitive::ArrayClear, vec![array.clone()]),
        (
            RuntimePrimitive::MapInsert,
            vec![map.clone(), Value::I32(3), Value::I32(4)],
        ),
        (
            RuntimePrimitive::MapRemove,
            vec![map.clone(), Value::I32(1)],
        ),
        (RuntimePrimitive::MapClear, vec![map.clone()]),
        (
            RuntimePrimitive::SetInsert,
            vec![set.clone(), Value::I32(2)],
        ),
        (
            RuntimePrimitive::SetRemove,
            vec![set.clone(), Value::I32(1)],
        ),
        (RuntimePrimitive::SetClear, vec![set.clone()]),
    ];
    for (op, args) in operations {
        let snapshot = || match &args[0] {
            Value::Array(id) => gc.array_snapshot(*id).unwrap(),
            Value::Set(id) => gc.set_snapshot(*id).unwrap(),
            Value::Map(id) => gc
                .map_snapshot(*id)
                .unwrap()
                .into_iter()
                .map(|(k, v)| Value::Tuple(vec![k, v]))
                .collect(),
            _ => unreachable!(),
        };
        let guard = gc.begin_collection_iteration(&args[0]).unwrap();
        let before = snapshot();
        let units = gc.stats().allocation_units;
        let error = invoke(&gc, op, &args).unwrap_err();
        assert_eq!(error.message(), "structural modification during iteration");
        assert_eq!(snapshot(), before);
        assert_eq!(gc.stats().allocation_units, units);
        drop(guard);
    }
    let Value::Array(id) = array else {
        unreachable!()
    };
    let guard = gc.begin_collection_iteration(&Value::Array(id)).unwrap();
    gc.array_set(id, 0, Value::I32(9)).unwrap();
    let nested = gc.begin_collection_iteration(&Value::Array(id)).unwrap();
    drop(nested);
    assert!(gc.array_push(id, Value::I32(2)).is_err());
    drop(guard);
    gc.array_push(id, Value::I32(2)).unwrap();
    let guard = gc.begin_collection_iteration(&map).unwrap();
    invoke(
        &gc,
        RuntimePrimitive::MapInsert,
        &[map, Value::I32(1), Value::I32(9)],
    )
    .unwrap();
    drop(guard);
    let guard = gc.begin_collection_iteration(&set).unwrap();
    invoke(&gc, RuntimePrimitive::SetInsert, &[set, Value::I32(1)]).unwrap();
    drop(guard);
}
