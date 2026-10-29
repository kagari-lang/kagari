use crate::{
    Runtime,
    native::storage::{
        scan::{NativeScanKind, ScanBudget, ScanStatus},
        value::StoredValue,
    },
    resource::HeapCharge,
    test_support::layouts::allocation_owner,
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
    assert_eq!(
        runtime.gc().stats().current_retained_bytes,
        128 * 1024 + 1024
    );
    assert!(runtime.gc().collection_due());
    let root = runtime.root_value(value).unwrap();
    runtime.gc_safepoint().unwrap();
    assert!(runtime.gc().validate_value(&value));
    assert!(!runtime.gc().collection_due());
    drop(root);
    let collection = runtime.collect_garbage().unwrap();
    assert_eq!(collection.reclaimed_objects, 1);
    assert_eq!(collection.reclaimed_retained_bytes, 128 * 1024 + 1024);
    assert_eq!(runtime.gc().stats().current_retained_bytes, 0);
    assert_eq!(runtime.gc().allocated_objects(), 0);
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

    unsafe impl NativePayload for Payload {
        type ScanState = ();
        const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

        fn begin_scan(&self) {}

        fn scan<'a>(
            &'a self,
            _: &mut Self::ScanState,
            budget: &mut ScanBudget,
            _: &mut dyn FnMut(&'a StoredValue),
        ) -> ScanStatus {
            budget.charge(1);
            ScanStatus::Complete
        }

        fn charge(&self) -> HeapCharge {
            let query = self.0.fetch_add(1, Ordering::Relaxed);
            // Deliberately drifting provider report: only admission is authoritative.
            HeapCharge::new(7 + query, query)
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
    assert_eq!(before.current_retained_bytes, 1024);
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

    unsafe impl NativePayload for Overflow {
        type ScanState = ();
        const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

        fn begin_scan(&self) {}

        fn scan<'a>(
            &'a self,
            _: &mut Self::ScanState,
            budget: &mut ScanBudget,
            _: &mut dyn FnMut(&'a StoredValue),
        ) -> ScanStatus {
            budget.charge(1);
            ScanStatus::Complete
        }

        fn charge(&self) -> HeapCharge {
            if self.0 {
                HeapCharge::new(usize::MAX, 0)
            } else {
                HeapCharge::new(0, usize::MAX)
            }
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
fn array_layouts_charge_capacity_not_values_or_zst_capacity_sentinel() {
    use crate::gc::array_storage::{ArrayPayload, ArrayStorage};
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    for (ty, value, width) in [
        (Ty::Builtin(BuiltinType::Unit), Value::Unit, 0),
        (
            Ty::Builtin(BuiltinType::Char),
            Value::Char('🙂'),
            size_of::<char>(),
        ),
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
            .with_native::<ArrayPayload, _>(id, |payload| Ok(payload.values.capacity()))
            .unwrap();
        let bytes = 1024 + capacity.checked_mul(width).unwrap();
        assert_eq!(heap.stats().current_retained_bytes, bytes as u128);
        runtime.collect_garbage().unwrap();
        assert_eq!(heap.stats().current_retained_bytes, 0);
    }
    // Traced slots have Value width even when their current values are scalar.
    let values = ArrayStorage::Traced(Vec::with_capacity(37));
    assert_eq!(
        values.retained_bytes(),
        values.capacity() * size_of::<Value>()
    );
}

#[test]
fn take_transfers_indirect_charge_once_and_retains_tombstone_backing() {
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

    unsafe impl NativePayload for Payload {
        type ScanState = ();
        const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

        fn begin_scan(&self) {}

        fn scan<'a>(
            &'a self,
            _: &mut Self::ScanState,
            budget: &mut ScanBudget,
            _: &mut dyn FnMut(&'a StoredValue),
        ) -> ScanStatus {
            budget.charge(1);
            ScanStatus::Complete
        }

        fn charge(&self) -> HeapCharge {
            self.queries.fetch_add(1, Ordering::Relaxed);
            HeapCharge::new(self.bytes.len(), self.bytes.capacity())
        }
    }

    // SAFETY: The payload contains only owned bytes and an edge-free counter;
    // its installed allocation and logical size never change.
    unsafe impl NativeTake for Payload {}

    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let queries = Arc::new(AtomicUsize::new(0));
    let bytes = Vec::with_capacity(4096);
    let expected = bytes.capacity() + 1024;
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
    assert_eq!(runtime.gc().stats().current_retained_bytes, 1024);
    assert_eq!(runtime.gc().stats().current_heap_units, 1);
    assert!(runtime.gc().take_native::<Payload>(id).is_err());
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(queries.load(Ordering::Relaxed), 1);
    assert_eq!(runtime.gc().stats().current_heap_units, 0);
    assert_eq!(payload.bytes.capacity(), expected - 1024);
    assert_eq!(runtime.gc().stats().current_retained_bytes, 0);
}
