use crate::{
    Runtime,
    gc::mutations::PreparedCollectionCommit,
    native::{binding::NativeResult, conversion::context::ConversionContext},
    resource::HeapCharge,
    test_support::layouts::sequence_owner,
    value::Value,
};

#[test]
fn reserved_empty_string_drives_automatic_collection() {
    let runtime = Runtime::default();
    let value = runtime
        .gc()
        .alloc_string(String::with_capacity(128 * 1024))
        .unwrap();
    assert_eq!(runtime.gc().stats().current_heap_units, 1);
    assert_eq!(runtime.gc().stats().current_retained_bytes, 128 * 1024);
    assert!(runtime.gc().collection_due());
    let root = runtime.root_value(value).unwrap();
    runtime.gc_safepoint().unwrap();
    assert!(runtime.gc().validate_value(&value));
    assert!(!runtime.gc().collection_due());
    drop(root);
    let collection = runtime.collect_garbage().unwrap();
    assert_eq!(collection.reclaimed_retained_bytes, 128 * 1024);
    assert_eq!(runtime.gc().stats().current_retained_bytes, 0);
    assert!(!runtime.gc().validate_value(&value));
}

#[test]
fn native_charge_is_queried_once_and_never_during_release() {
    use crate::{
        native::storage::{NativePayload, NativeStorage},
        test_support::layouts::allocation_owner,
    };
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Debug)]
    struct Payload(Arc<AtomicUsize>);

    impl NativePayload for Payload {
        fn trace<'a>(&'a self, _: &mut dyn FnMut(&'a Value)) {}

        fn charge(&self) -> NativeResult<HeapCharge> {
            let query = self.0.fetch_add(1, Ordering::Relaxed);
            // Deliberately drifting provider report: only admission is authoritative.
            Ok(HeapCharge::new(7 + query, query))
        }
    }

    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let queries = Arc::new(AtomicUsize::new(0));
    let object = NativeStorage::payload::<Payload>()
        .prepare_payload(
            runtime.gc(),
            &Ty::Builtin(BuiltinType::Unit),
            Payload(queries.clone()),
            &owner,
        )
        .unwrap();
    let id = runtime.gc().alloc_native(object).unwrap();
    let before = runtime.gc().stats();
    assert_eq!(before.current_retained_bytes, size_of::<Payload>() as u128);
    runtime
        .gc()
        .with_native::<Payload, _>(id, |_| Ok(()))
        .unwrap();
    assert_eq!(queries.load(Ordering::Relaxed), 1);
    runtime.collect_garbage().unwrap();
    assert_eq!(queries.load(Ordering::Relaxed), 1);
    assert_eq!(runtime.gc().stats().current_heap_units, 0);
    assert_eq!(runtime.gc().stats().current_retained_bytes, 0);
}

#[test]
fn rejected_provider_charge_never_publishes_storage_or_counters() {
    use crate::{
        native::storage::{NativePayload, NativeStorage},
        test_support::layouts::allocation_owner,
    };
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    #[derive(Debug)]
    struct Overflow(bool);

    impl NativePayload for Overflow {
        fn trace<'a>(&'a self, _: &mut dyn FnMut(&'a Value)) {}

        fn charge(&self) -> NativeResult<HeapCharge> {
            Ok(if self.0 {
                HeapCharge::new(usize::MAX, 0)
            } else {
                HeapCharge::new(0, usize::MAX)
            })
        }
    }

    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    for units in [true, false] {
        let before = runtime.gc().stats();
        let result = NativeStorage::payload::<Overflow>()
            .prepare_payload(
                runtime.gc(),
                &Ty::Builtin(BuiltinType::Unit),
                Overflow(units),
                &owner,
            )
            .and_then(|object| runtime.gc().alloc_native(object));
        assert!(result.is_err());
        assert_eq!(runtime.gc().stats(), before);
        assert!(!runtime.is_quarantined());
    }
}

#[test]
fn native_revision_survives_relocation_replacement_and_rejects_exhaustion() {
    let mut runtime = Runtime::default();
    let owner = sequence_owner(&mut runtime);
    let mut context = ConversionContext::new(&runtime, &owner).unwrap();
    let garbage = context.encode(vec![0_i32]).unwrap();
    let stale = garbage.value(runtime.gc()).unwrap();
    drop(garbage);
    let root = context.encode(vec![1_i32]).unwrap();
    let value = root.value(runtime.gc()).unwrap();
    let Value::GcHandle(id) = value else {
        panic!("native sequence");
    };
    let heap = runtime.gc();
    heap.sequence_push(id, Value::I32(2)).unwrap();
    let revision = heap.collection_revision(&value).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(heap.collection_revision(&value), Some(revision));
    assert_eq!(heap.collection_revision(&stale), None);

    let copy = heap
        .root_value(Value::GcHandle(heap.clone_buffer(id).unwrap()))
        .unwrap();
    assert_eq!(
        heap.collection_revision(&copy.value(heap).unwrap()),
        Some(0)
    );
    let fresh = context.encode(vec![9_i32]).unwrap();
    assert_eq!(
        heap.collection_revision(&fresh.value(heap).unwrap()),
        Some(0)
    );
    assert_eq!(heap.collection_revision(&stale), None);
    let buffer = context.encode(vec![5_i32]).unwrap();
    heap.commit_prepared_collection(
        PreparedCollectionCommit::ReplaceSequence,
        &[value, buffer.value(heap).unwrap()],
    )
    .unwrap();
    assert_eq!(heap.collection_revision(&value), Some(revision + 1));
    assert!(heap.check_collection_revision(&value, revision).is_err());
    assert_eq!(heap.sequence_snapshot(id).unwrap(), vec![Value::I32(5)]);

    {
        let mut objects = heap.objects.borrow_mut();
        heap.native_mut(&mut objects, id).unwrap().revision = u64::MAX;
    }
    let units = heap.stats().current_heap_units;
    assert!(heap.sequence_push(id, Value::I32(6)).is_err());
    assert_eq!(heap.collection_revision(&value), Some(u64::MAX));
    assert_eq!(heap.sequence_snapshot(id).unwrap(), vec![Value::I32(5)]);
    assert_eq!(heap.stats().current_heap_units, units);
}

#[test]
fn byte_threshold_is_configurable_and_none_disables_automatic_collection() {
    use crate::{RuntimeConfig, gc::GcHeapConfig};
    for enabled in [false, true] {
        let runtime = Runtime::new(RuntimeConfig {
            gc: GcHeapConfig {
                collection_threshold: enabled.then_some(usize::MAX),
                retained_byte_threshold: 128,
            },
            ..Default::default()
        });
        let value = runtime
            .gc()
            .alloc_string(String::with_capacity(256))
            .unwrap();
        assert_eq!(runtime.gc().collection_due(), enabled);
        runtime.gc_safepoint().unwrap();
        assert_eq!(runtime.gc().validate_value(&value), !enabled);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc().stats().current_retained_bytes, 0);
    }
}

#[test]
fn sequence_reserve_clear_failed_growth_and_partial_edit_keep_committed_charge() {
    use crate::error::RuntimeError;
    let mut runtime = Runtime::default();
    let owner = sequence_owner(&mut runtime);
    let root = ConversionContext::new(&runtime, &owner)
        .unwrap()
        .encode(vec![1i32, 2, 3])
        .unwrap();
    let heap = runtime.gc();
    let value = root.value(heap).unwrap();
    let Value::GcHandle(id) = value else {
        panic!("sequence")
    };
    let before = heap.stats();
    let old_capacity = heap.collection_capacity(&value).unwrap();
    heap.reserve_collection(&value, 1024).unwrap();
    let capacity = heap.collection_capacity(&value).unwrap();
    let reserved = heap.stats();
    assert_eq!(reserved.current_heap_units, before.current_heap_units);
    assert_eq!(
        reserved.current_retained_bytes - before.current_retained_bytes,
        ((capacity - old_capacity) * size_of::<i32>()) as u128
    );
    assert!(heap.reserve_collection(&value, usize::MAX).is_err());
    assert_eq!(heap.stats(), reserved);
    assert!(heap.sequence_push(id, Value::Bool(true)).is_err());
    assert_eq!(heap.stats(), reserved);
    let error = heap.edit_sequence(id, |mut edit| {
        runtime.collect_garbage().unwrap();
        assert_eq!(
            heap.stats().current_retained_bytes,
            reserved.current_retained_bytes
        );
        edit.retain(|value| {
            if value == Value::I32(3) {
                return Err(RuntimeError::module_validation("predicate failure"));
            }
            Ok(value != Value::I32(1))
        })
    });
    assert!(error.is_err());
    assert_eq!(
        heap.sequence_snapshot(id).unwrap(),
        vec![Value::I32(2), Value::I32(3)]
    );
    assert_eq!(
        heap.stats().current_heap_units,
        reserved.current_heap_units - 1
    );
    assert_eq!(
        heap.stats().current_retained_bytes,
        reserved.current_retained_bytes
    );
    heap.sequence_clear(id).unwrap();
    assert_eq!(heap.stats().current_heap_units, 1);
    assert_eq!(
        heap.stats().current_retained_bytes,
        reserved.current_retained_bytes
    );
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(heap.stats().current_heap_units, 0);
    assert_eq!(heap.stats().current_retained_bytes, 0);
}

#[test]
fn sequence_layouts_charge_capacity_not_values_or_zst_capacity_sentinel() {
    use crate::native::sequence::{SequencePayload, SequenceStorage};
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    let mut runtime = Runtime::default();
    let owner = sequence_owner(&mut runtime);
    for (ty, value, width) in [
        (Ty::Builtin(BuiltinType::Unit), Value::Unit, 0),
        (Ty::Builtin(BuiltinType::I8), Value::I32(1), size_of::<i8>()),
        (
            Ty::Builtin(BuiltinType::I64),
            Value::I64(1),
            size_of::<i64>(),
        ),
    ] {
        let heap = runtime.gc();
        let id = runtime.alloc_array(&owner, ty, vec![value; 3]).unwrap();
        let capacity = heap
            .with_native::<SequencePayload, _>(id, |payload| Ok(payload.values.capacity()))
            .unwrap();
        let bytes = size_of::<SequencePayload>() + capacity.checked_mul(width).unwrap();
        assert_eq!(heap.stats().current_retained_bytes, bytes as u128);
        runtime.collect_garbage().unwrap();
        assert_eq!(heap.stats().current_retained_bytes, 0);
    }
    // Traced slots have Value width even when their current values are scalar.
    let values = SequenceStorage::Traced(Vec::with_capacity(37));
    assert_eq!(
        values.retained_bytes(),
        values.capacity() * size_of::<Value>()
    );
}

#[test]
fn take_releases_recorded_backing_once_and_keeps_the_inert_identity() {
    use crate::{
        native::{
            payload::take::NativeTake,
            storage::{NativePayload, NativeStorage},
        },
        test_support::layouts::allocation_owner,
    };
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[derive(Debug)]
    struct Payload {
        bytes: Vec<u8>,
        queries: Arc<AtomicUsize>,
    }

    impl NativePayload for Payload {
        fn trace<'a>(&'a self, _: &mut dyn FnMut(&'a Value)) {}

        fn charge(&self) -> NativeResult<HeapCharge> {
            self.queries.fetch_add(1, Ordering::Relaxed);
            Ok(HeapCharge::new(self.bytes.len(), self.bytes.capacity()))
        }
    }

    // SAFETY: The payload contains only owned bytes and an edge-free counter;
    // its installed allocation and logical size never change.
    unsafe impl NativeTake for Payload {}

    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let queries = Arc::new(AtomicUsize::new(0));
    let bytes = Vec::with_capacity(4096);
    let expected = bytes.capacity() + size_of::<Payload>();
    let object = NativeStorage::takeable::<Payload>()
        .prepare_payload(
            runtime.gc(),
            &Ty::Builtin(BuiltinType::Unit),
            Payload {
                bytes,
                queries: queries.clone(),
            },
            &owner,
        )
        .unwrap();
    let id = runtime.gc().alloc_native(object).unwrap();
    let root = runtime.root_value(Value::GcHandle(id)).unwrap();
    assert_eq!(
        runtime.gc().stats().current_retained_bytes,
        expected as u128
    );
    let payload = runtime.gc().take_native::<Payload>(id).unwrap();
    assert_eq!(queries.load(Ordering::Relaxed), 1);
    assert_eq!(runtime.gc().stats().current_retained_bytes, 0);
    assert_eq!(runtime.gc().stats().current_heap_units, 1);
    assert!(runtime.gc().take_native::<Payload>(id).is_err());
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(queries.load(Ordering::Relaxed), 1);
    assert_eq!(runtime.gc().stats().current_heap_units, 0);
    assert_eq!(payload.bytes.capacity(), expected - size_of::<Payload>());
}

#[test]
fn hash_mutations_and_partial_reservation_update_the_recorded_charge() {
    use crate::{
        error::RuntimeError,
        native::{
            hash_storage::{HashMapStorage, HashSetStorage},
            hashed::{MapPayload, SetPayload},
            storage::NativeStorage,
            storage_type::StorageType,
        },
        test_support::layouts::allocation_owner,
    };
    use kagari_types::{collection::CollectionAccess, scalar::BuiltinType, ty::Ty};
    use std::sync::Arc;
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let element = Ty::Builtin(BuiltinType::I32);
    let contract = Arc::new(StorageType::prepare(element.clone(), &owner).unwrap());
    for builtin in [true, false] {
        let heap = runtime.gc();
        let object = NativeStorage::map(0, 1)
            .prepare_payload(
                heap,
                &Ty::Map {
                    access: CollectionAccess::Mutable,
                    key: Box::new(element.clone()),
                    value: Box::new(element.clone()),
                },
                MapPayload {
                    key: contract.clone(),
                    value: contract.clone(),
                    builtin_keys: builtin,
                    entries: HashMapStorage::new(),
                },
                &owner,
            )
            .unwrap();
        let id = heap.alloc_native(object).unwrap();
        let value = Value::Map(id);
        let root = runtime.root_value(value).unwrap();
        if builtin {
            heap.map_insert(id, Value::I32(1), Value::I32(2)).unwrap();
            heap.map_insert(id, Value::I32(1), Value::I32(3)).unwrap();
        } else {
            heap.custom_insert(&value, 7, -1, Value::I32(1), Value::I32(2))
                .unwrap();
            heap.custom_insert(&value, 7, -1, Value::I32(2), Value::I32(3))
                .unwrap();
        }
        let expected_units = if builtin { 2 } else { 3 };
        assert_eq!(heap.stats().current_heap_units, expected_units);
        let before = heap.stats().current_retained_bytes;
        // Model reserve success followed by a later preparation failure. Semantic
        // contents stay unchanged, but retained allocation must not disappear.
        let result = heap
            .with_map_mut(id, 0, |entries| {
                entries.try_reserve(4096).unwrap();
                Err::<(), _>(RuntimeError::resource_limit("later preparation"))
            })
            .unwrap();
        assert!(result.is_err());
        assert_eq!(heap.stats().current_heap_units, expected_units);
        assert!(heap.stats().current_retained_bytes > before);
        let expected_bytes =
            heap.with_map(id, HashMapStorage::pressure_bytes).unwrap() + size_of::<MapPayload>();
        assert_eq!(heap.stats().current_retained_bytes, expected_bytes as u128);
        heap.map_clear(id).unwrap();
        let expected_bytes =
            heap.with_map(id, HashMapStorage::pressure_bytes).unwrap() + size_of::<MapPayload>();
        assert_eq!(heap.stats().current_heap_units, 1);
        assert_eq!(heap.stats().current_retained_bytes, expected_bytes as u128);
        drop(root);
        runtime.collect_garbage().unwrap();
        assert_eq!(heap.stats().current_heap_units, 0);
        assert_eq!(heap.stats().current_retained_bytes, 0);

        let object = NativeStorage::set(0)
            .prepare_payload(
                heap,
                &Ty::Set(Box::new(element.clone()), CollectionAccess::Mutable),
                SetPayload {
                    element: contract.clone(),
                    builtin_keys: builtin,
                    entries: HashSetStorage::new(),
                },
                &owner,
            )
            .unwrap();
        let id = heap.alloc_native(object).unwrap();
        let value = Value::Set(id);
        if builtin {
            heap.set_insert(id, Value::I32(1)).unwrap();
            heap.set_insert(id, Value::I32(1)).unwrap();
        } else {
            heap.custom_insert(&value, 7, -1, Value::I32(1), Value::Unit)
                .unwrap();
        }
        assert_eq!(heap.stats().current_heap_units, 2);
        heap.reserve_collection(&value, 512).unwrap();
        heap.set_clear(id).unwrap();
        assert_eq!(heap.stats().current_heap_units, 1);
        assert!(heap.stats().current_retained_bytes > size_of::<SetPayload>() as u128);
        runtime.collect_garbage().unwrap();
        assert_eq!(heap.stats().current_retained_bytes, 0);
    }
}

#[test]
fn detached_cleanup_releases_logical_units_after_termination_or_quarantine() {
    for quarantine in [false, true] {
        let mut runtime = Runtime::default();
        let owner = sequence_owner(&mut runtime);
        let root = ConversionContext::new(&runtime, &owner)
            .unwrap()
            .encode(vec![1i32, 2])
            .unwrap();
        let heap = runtime.gc();
        let Value::GcHandle(id) = root.value(heap).unwrap() else {
            panic!("sequence")
        };
        let before = heap.stats();
        let _session = runtime.begin_execution(&owner, Default::default()).unwrap();
        assert!(
            heap.edit_sequence(id, |mut edit| {
                edit.retain(|_| Ok(false))?;
                Err::<(), _>(if quarantine {
                    heap.resources.quarantine("test cleanup")
                } else {
                    heap.resource_limit("test cleanup")
                })
            })
            .is_err()
        );
        assert!(heap.ensure_execution_allowed().is_err());
        assert_eq!(
            heap.stats().current_heap_units,
            before.current_heap_units - 2
        );
        assert_eq!(
            heap.stats().current_retained_bytes,
            before.current_retained_bytes
        );
    }
}
