use crate::tests::execution::native_boundary::compile;
use kagari_runtime::{
    error::RuntimeErrorKind,
    gc::{GcObjectKind, HeapObjectId},
    module::entry::ProgramEntry,
    range::index_bound,
    reflection,
    value::{Value, ValueCategory},
};

use std::ops::Bound;

#[test]
fn index_bounds_validate_the_applied_type_even_without_a_payload() {
    let (mut vm, loaded) = compile(
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
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let valid = vm
        .execute_module(&loaded, "valid", vm.execution_options())
        .unwrap();
    assert_eq!(
        index_bound(
            vm.gc(),
            &valid
                .return_value
                .value(vm.gc())
                .expect("retained execution result")
        )
        .unwrap(),
        Bound::Unbounded
    );
    for (name, expected) in [
        ("included", Bound::Included(u64::MAX)),
        ("excluded", Bound::Excluded(1_u64 << 32)),
    ] {
        let report = vm
            .execute_module(&loaded, name, vm.execution_options())
            .unwrap();
        assert_eq!(
            index_bound(vm.gc(), &report.return_value.value(vm.gc()).unwrap()).unwrap(),
            expected
        );
    }
    for name in ["wrong_argument", "wrong_owner"] {
        let invalid = vm
            .execute_module(&loaded, name, vm.execution_options())
            .unwrap();
        assert_eq!(
            index_bound(
                vm.gc(),
                &invalid
                    .return_value
                    .value(vm.gc())
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
    let (mut vm, program) = compile(
        include_str!("../../../../tests/fixtures/native/gc_identity.kgr"),
        None,
    );
    let loaded = vm.select_program(&program).unwrap();
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    let heap = vm.gc();
    let Value::Tuple(values) = report.return_value.value(heap).unwrap() else {
        panic!("tuple")
    };
    let values = heap.tuple(values).unwrap().to_vec();
    let [Value::Array(first), Value::Struct(second)] = values.as_slice() else {
        panic!("handles")
    };
    assert_ne!(first, second);
    assert_eq!(heap.object_kind(*first), Some(GcObjectKind::Array));
    assert_eq!(heap.object_kind(*second), Some(GcObjectKind::Struct));
    for value in [Value::Array(*first), Value::Struct(*second)] {
        assert_eq!(value.category(), ValueCategory::ScriptOwned);
        assert!(value.is_default_heap_payload(heap));
    }
    let Value::Str(name) = reflection::type_of(heap, &Value::Array(*first)).unwrap() else {
        panic!("type name")
    };
    assert_eq!(&*heap.string(name).unwrap(), "array");
}

#[test]
fn root_scanning_traces_only_gc_managed_boundaries() {
    let (mut vm, program) = compile(
        include_str!("../../../../tests/fixtures/native/gc_root_boundaries.kgr"),
        None,
    );
    let loaded = vm.select_program(&program).unwrap();
    let Value::Tuple(values) = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .unwrap()
    else {
        panic!("tuple")
    };
    let heap = vm.gc();
    let values = heap.tuple(values).unwrap().to_vec();
    let [Value::Struct(record), Value::Array(strings)] = values.as_slice() else {
        panic!("handles")
    };
    let layout = heap.struct_layout(*record).unwrap();
    let Value::Array(leaf) = heap.struct_get_slot(*record, &layout, 0).unwrap() else {
        panic!("leaf array")
    };
    let Value::Str(name) = heap.struct_get_slot(*record, &layout, 1).unwrap() else {
        panic!("name")
    };
    let Value::Str(seen) = heap.array_get(*strings, 0).unwrap() else {
        panic!("seen")
    };
    let root = heap
        .root_value(
            heap.alloc_tuple(vec![
                Value::Struct(*record),
                Value::Array(*strings),
                Value::Unit,
            ])
            .unwrap(),
        )
        .unwrap();
    let Value::Tuple(tuple) = root.value(heap).unwrap() else {
        panic!("root tuple")
    };
    assert_eq!(
        heap.trace_roots().unwrap(),
        vec![tuple, *record, leaf, name, *strings, seen]
    );
    root.set(heap, Value::Array(leaf)).unwrap();
    assert_eq!(heap.trace_roots().unwrap(), vec![leaf]);
    assert_eq!(root.value(heap).unwrap(), Value::Array(leaf));
    drop(root);
    assert_eq!(heap.trace_roots().unwrap(), Vec::<HeapObjectId>::new());
}

#[test]
fn root_scanning_handles_cycles_without_duplicate_identity() {
    let (mut vm, program) = compile(
        include_str!("../../../../tests/fixtures/native/gc_cycle_roots.kgr"),
        None,
    );
    let loaded = vm.select_program(&program).unwrap();
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    let heap = vm.gc();
    let Value::Struct(record) = report.return_value.value(heap).unwrap() else {
        panic!("record")
    };
    let layout = heap.struct_layout(record).unwrap();
    let Value::Enum(next) = heap.struct_get_slot(record, &layout, 0).unwrap() else {
        panic!("cycle edge")
    };
    assert_eq!(heap.trace_roots().unwrap(), vec![record, next]);
}

#[test]
fn custom_set_absent_removal_does_not_invalidate_iteration() {
    let (mut vm, loaded) = compile(
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
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    assert_eq!(vm.gc().active_roots(), 0);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc().allocated_objects(), 0);
}
