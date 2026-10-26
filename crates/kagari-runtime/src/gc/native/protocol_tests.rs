//! Provider contracts: one scanner, short loans, retained extraction and sealed recovery.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::{HeapObjectId, mutations::PreparedCollectionCommit},
    native::{
        conversion::context::ConversionContext,
        storage::{
            NativePayload, NativeStorage,
            access::EditPlan,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
        },
        storage_type::StorageType,
        views::{NativeOperationGuard, NativeOperationKind},
    },
    resource::HeapCharge,
    test_support::layouts::{allocation_owner, sequence_owner},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Debug)]
struct Slots(Vec<Value>);

// SAFETY: All strong edges are the indexed Vec slots; the cursor has only numeric
// indices. Edits in these tests use checked services or already admitted edges.
unsafe impl NativePayload for Slots {
    type ScanState = (usize, usize);
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        (0, self.0.len())
    }

    fn scan<'a>(
        &'a self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        while state.0 < state.1 {
            if !budget.take(1) {
                return ScanStatus::Pending;
            }
            if let Some(value) = self.0.get(state.0) {
                visit(value);
            }
            state.0 += 1;
        }
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(self.0.len(), self.0.capacity() * size_of::<Value>())
    }
}

fn allocate(
    runtime: &Runtime,
    owner: &crate::module::ExecutableModule,
    values: Vec<Value>,
) -> HeapObjectId {
    let object = NativeStorage::payload::<Slots>()
        .prepare_payload(
            runtime.gc(),
            &Ty::Builtin(BuiltinType::Unit),
            Slots(values),
            owner,
        )
        .unwrap();
    runtime.gc().alloc_native(object).unwrap()
}

#[test]
fn operation_policy_covers_sequence_facades_without_hiding_type_metadata() {
    let mut runtime = Runtime::default();
    let owner = sequence_owner(&mut runtime);
    let mut conversion = ConversionContext::new(&runtime, &owner).unwrap();
    let root = conversion.encode(vec![10_i32, 20]).unwrap();
    let target = conversion.encode(vec![30_i32, 40]).unwrap();
    let heap = runtime.gc();
    let Value::GcHandle(id) = root.value(heap).unwrap() else {
        panic!("sequence")
    };
    for kind in [
        NativeOperationKind::ExclusiveSlots,
        NativeOperationKind::ReadOnlyCallbacks,
    ] {
        let guard = heap.try_enter_native_operation(id, kind).unwrap().unwrap();
        assert!(heap.sequence_contract(id).is_some());
        assert!(heap.validate_value(&Value::GcHandle(id)));
        assert!(heap.sequence_set(id, 0, Value::I32(99)).is_err());
        assert!(heap.with_buffer_mut(id, 0, |_| ()).is_err());
        if kind == NativeOperationKind::ExclusiveSlots {
            assert_eq!(heap.sequence_len(id), None);
            assert_eq!(heap.sequence_get(id, 0), None);
            assert_eq!(heap.sequence_snapshot(id), None);
            assert!(heap.sequence_element(id, 0).is_err());
            assert!(conversion.decode::<Vec<i32>>(&root).is_err());
            assert!(
                heap.commit_prepared_collection(
                    PreparedCollectionCommit::ReplaceSequence,
                    &[target.value(heap).unwrap(), root.value(heap).unwrap()],
                )
                .is_err()
            );
        } else {
            assert_eq!(heap.sequence_len(id), Some(2));
            assert_eq!(heap.sequence_get(id, 0), Some(Value::I32(10)));
            assert_eq!(
                heap.sequence_snapshot(id),
                Some(vec![Value::I32(10), Value::I32(20)])
            );
            assert_eq!(conversion.decode::<Vec<i32>>(&root).unwrap(), vec![10, 20]);
            heap.commit_prepared_collection(
                PreparedCollectionCommit::ReplaceSequence,
                &[target.value(heap).unwrap(), root.value(heap).unwrap()],
            )
            .unwrap();
            assert_eq!(
                conversion.decode::<Vec<i32>>(&target).unwrap(),
                vec![10, 20]
            );
        }
        drop(guard);
        assert_eq!(conversion.decode::<Vec<i32>>(&root).unwrap(), vec![10, 20]);
    }
}

#[test]
fn operation_policy_blocks_iterator_receiver_before_preview_or_finish() {
    let mut runtime = Runtime::default();
    let owner = sequence_owner(&mut runtime);
    let mut conversion = ConversionContext::new(&runtime, &owner).unwrap();
    let source = conversion.encode(vec![10_i32, 20]).unwrap();
    let source_type = conversion.type_for::<Vec<i32>>().unwrap();
    let item_type = conversion.type_for::<i32>().unwrap();
    let heap = runtime.gc();
    let cursor = heap
        .new_iter(
            &source.value(heap).unwrap(),
            &source_type,
            item_type,
            &owner,
        )
        .unwrap();
    let _root = runtime.root_value(cursor).unwrap();
    let Value::GcHandle(id) = cursor else {
        panic!("iterator")
    };
    let ty = Ty::Iter(Box::new(Ty::Builtin(BuiltinType::I32)));
    for kind in [
        NativeOperationKind::ExclusiveSlots,
        NativeOperationKind::ReadOnlyCallbacks,
    ] {
        let guard = heap.try_enter_native_operation(id, kind).unwrap().unwrap();
        let mut finished = false;
        assert!(
            heap.advance_iter_with(&cursor, &ty, |_| {
                finished = true;
                Ok(())
            })
            .is_err()
        );
        assert!(
            !finished,
            "denied advancement must not run result construction"
        );
        drop(guard);
    }
    let mut guard = None;
    assert!(
        heap.advance_iter_with(&cursor, &ty, |_| {
            guard = heap.try_enter_native_operation(id, NativeOperationKind::ReadOnlyCallbacks)?;
            Ok(())
        })
        .is_err(),
        "commit must recheck protection acquired during result construction"
    );
    drop(guard);
    assert_eq!(
        heap.advance_iter_with(&cursor, &ty, Ok).unwrap(),
        Some(Value::I32(10))
    );
    assert_eq!(
        heap.advance_iter_with(&cursor, &ty, Ok).unwrap(),
        Some(Value::I32(20))
    );
}

#[test]
fn general_edits_reconcile_completed_changes_on_error_and_unwind() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let id = allocate(&runtime, &owner, vec![Value::Unit]);
    let root = runtime.root_value(Value::GcHandle(id)).unwrap();
    let heap = runtime.gc();
    for panic in [false, true] {
        let before = heap.stats().current_heap_units;
        let result = catch_unwind(AssertUnwindSafe(|| {
            heap.edit_native::<Slots, ()>(
                &runtime,
                id,
                None,
                EditPlan {
                    max_logical_units: 8,
                    structural: true,
                },
                |payload, access| {
                    assert!(runtime.collect_garbage().is_err());
                    assert!(heap.with_native::<Slots, _>(id, |_| Ok(())).is_err());
                    payload.0.reserve(1);
                    access.commit_structure();
                    payload.0.push(Value::Unit);
                    if panic {
                        panic!("completed native write");
                    }
                    Err(RuntimeError::module_validation("completed native write"))
                },
            )
        }));
        assert!(if panic {
            result.is_err()
        } else {
            result.unwrap().is_err()
        });
        assert_eq!(heap.stats().current_heap_units, before + 1);
        heap.with_native::<Slots, _>(id, |payload| {
            let objects = heap.objects.borrow();
            let object = heap.native_ref(&objects, id).unwrap();
            assert_eq!(object.charge, object.storage_charge(payload.charge()));
            assert_eq!(object.revision, payload.0.len() as u64 - 1);
            Ok(())
        })
        .unwrap();
    }
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(heap.stats().current_heap_units, 0);
    assert_eq!(heap.stats().current_retained_bytes, 0);
}

#[test]
fn retained_extraction_survives_replacement_and_checks_publication_type_and_owner() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let argument = runtime
        .type_arguments(&owner, None, &[Ty::Builtin(BuiltinType::String)])
        .unwrap()
        .remove(0);
    let contract = StorageType::prepare_scoped(argument, &owner).unwrap();
    let value = runtime.gc().alloc_string("retained".into()).unwrap();
    let id = allocate(&runtime, &owner, vec![value]);
    let root = runtime.root_value(Value::GcHandle(id)).unwrap();
    let retained = runtime
        .gc()
        .read_native::<Slots, _>(&runtime, id, None, |payload, access| {
            access.retain(&contract, &payload.0[0])
        })
        .unwrap();
    runtime
        .gc()
        .edit_native::<Slots, _>(
            &runtime,
            id,
            None,
            EditPlan {
                max_logical_units: 1,
                structural: true,
            },
            |payload, access| {
                access.commit_structure();
                payload.0.clear();
                Ok(())
            },
        )
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&value));
    runtime
        .gc()
        .edit_native::<Slots, _>(
            &runtime,
            id,
            None,
            EditPlan {
                max_logical_units: 1,
                structural: true,
            },
            |payload, access| {
                assert!(access.retain(&contract, &Value::Bool(true)).is_err());
                let value = access.publish(&contract, &retained)?;
                payload.0.reserve(1);
                access.commit_structure();
                payload.0.push(value);
                Ok(())
            },
        )
        .unwrap();
    drop(retained);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&value));
    drop(root);
    runtime.collect_garbage().unwrap();
    assert!(!runtime.gc().validate_value(&value));
}

#[test]
fn operation_guard_blocks_aliases_and_recovers_after_execution_is_closed() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let id = allocate(&runtime, &owner, vec![Value::Unit, Value::Unit]);
    let heap = runtime.gc();
    let readonly = NativeOperationGuard {
        heap,
        id,
        runtime: Some(&runtime),
        recovery: None,
        _root: runtime.root_value(Value::GcHandle(id)).unwrap(),
        _lease: heap
            .try_enter_native_operation(id, NativeOperationKind::ReadOnlyCallbacks)
            .unwrap()
            .unwrap(),
    };
    let nested = heap
        .try_enter_native_operation(id, NativeOperationKind::ReadOnlyCallbacks)
        .unwrap()
        .unwrap();
    assert!(heap.with_native::<Slots, _>(id, |_| Ok(())).is_ok());
    assert!(
        heap.edit_native::<Slots, _>(
            &runtime,
            id,
            Some(&readonly),
            EditPlan {
                max_logical_units: 2,
                structural: false
            },
            |_, _| Ok(())
        )
        .is_err()
    );
    drop(nested);
    drop(readonly);
    heap.prepare_native_recovery(id).unwrap();
    let guard = NativeOperationGuard {
        heap,
        id,
        runtime: Some(&runtime),
        recovery: Some(Box::new(|payload, access| {
            let slots = payload.downcast_mut::<Slots>().unwrap();
            slots.0.truncate(1);
            // SAFETY: This edge was admitted and remained in the same traced Vec.
            unsafe {
                access.republish_existing(&slots.0[0]);
            }
            access.commit_structure();
        })),
        _root: runtime.root_value(Value::GcHandle(id)).unwrap(),
        _lease: heap
            .try_enter_native_operation(id, NativeOperationKind::ExclusiveSlots)
            .unwrap()
            .unwrap(),
    };
    assert!(heap.with_native::<Slots, _>(id, |_| Ok(())).is_err());
    assert!(
        heap.read_native::<Slots, _>(&runtime, id, None, |_, _| Ok(()))
            .is_err()
    );
    assert!(
        heap.read_native::<Slots, _>(&runtime, id, Some(&guard), |payload, _| Ok(payload.0.len()))
            .is_ok()
    );
    assert!(
        heap.try_enter_native_operation(id, NativeOperationKind::ReadOnlyCallbacks)
            .unwrap()
            .is_none()
    );
    runtime
        .resources()
        .quarantine("test closes ordinary execution before cleanup");
    drop(guard);
    let objects = heap.objects.borrow();
    let object = heap.native_ref(&objects, id).unwrap();
    assert_eq!(object.payload::<Slots>().unwrap().0.len(), 1);
    assert_eq!(object.charge.logical_units, 1);
    assert_eq!(object.revision, 1);
    assert!(!heap.native_operations.is_active(id));
    assert_eq!(heap.stats().current_heap_units, 2);
}
