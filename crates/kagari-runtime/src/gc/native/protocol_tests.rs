//! Provider contracts: one scanner, short loans, retained extraction and sealed recovery.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::{graph::objects::ScanEdge, metrics::IndivisibleKind, reference::GcRef},
    native::{
        conversion::context::ConversionContext,
        payload::read::ReadReceiver,
        storage::{
            NativePayload, NativeStorage,
            access::EditPlan,
            scan::{NativeScanKind, ScanBudget, ScanEntries, ScanStatus},
            value::StoredValue,
        },
        storage_type::StorageType,
        views::{NativeOperationGuard, NativeOperationKind},
    },
    resource::HeapCharge,
    test_support::layouts::allocation_owner,
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    any::TypeId,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug)]
struct Slots(Vec<StoredValue>);

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
        visit: &mut dyn FnMut(&'a StoredValue),
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
) -> GcRef {
    let object = NativeStorage::payload::<Slots>()
        .prepare_payload(
            runtime.gc(),
            &Ty::Builtin(BuiltinType::Unit),
            Slots(values.into_iter().map(StoredValue::new).collect()),
            owner,
        )
        .unwrap();
    runtime.gc().alloc_native(object).unwrap()
}

#[derive(Debug)]
struct WideCursorPayload;

#[derive(Clone, Copy)]
#[repr(align(256))]
struct WideCursor([usize; 128]);

// SAFETY: Edge-free payload; the over-aligned Copy state has no borrowed data.
unsafe impl NativePayload for WideCursorPayload {
    type ScanState = WideCursor;
    const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

    fn begin_scan(&self) -> WideCursor {
        WideCursor([7; 128])
    }

    fn scan<'a>(
        &'a self,
        state: &mut WideCursor,
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'a StoredValue),
    ) -> ScanStatus {
        assert_eq!(state.0, [7; 128]);
        budget.charge(1);
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::default()
    }
}

#[test]
fn payload_admission_and_inspection_preserve_a_suspended_heap_scan_workspace() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let values = (0..3)
        .map(|i| heap.alloc_string(i.to_string()).unwrap())
        .collect::<Vec<_>>();
    let id = allocate(&runtime, &owner, values.clone());
    let entries = ScanEntries::new::<Slots>();
    let mut visited = Vec::new();
    heap.with_native::<Slots, _>(id, |payload| {
        let mut workspace = heap.native_scan_workspace.borrow_mut();
        entries.begin(payload, &mut workspace)?;
        assert_eq!(
            entries.resume(payload, &mut workspace, &mut ScanBudget::new(1), &mut |v| {
                visited.push(v.value())
            })?,
            ScanStatus::Pending
        );
        Ok(())
    })
    .unwrap();
    let object = NativeStorage::payload::<WideCursorPayload>()
        .prepare_payload(
            heap,
            &Ty::Builtin(BuiltinType::Unit),
            WideCursorPayload,
            &owner,
        )
        .unwrap();
    let wide = heap.alloc_native(object).unwrap();
    assert!(heap.validate_value(&Value::GcHandle(wide)));
    // Both inspection clients previously shared and overwrote the collector state.
    assert_eq!(heap.trace_value(&Value::GcHandle(id)).unwrap().len(), 4);
    heap.validate_async_values(&[Value::GcHandle(id)]).unwrap();
    heap.with_native::<Slots, _>(id, |payload| {
        let mut workspace = heap.native_scan_workspace.borrow_mut();
        while entries.resume(payload, &mut workspace, &mut ScanBudget::new(1), &mut |v| {
            visited.push(v.value())
        })? == ScanStatus::Pending
        {}
        Ok(())
    })
    .unwrap();
    assert_eq!(
        visited, values,
        "growth and inspection must not restart or replace the cursor"
    );
    let result = runtime.collect_garbage().unwrap();
    assert_eq!(result.reclaimed_objects, 5);
}

#[derive(Debug)]
struct TraceFault<const INDIVISIBLE: bool>(Arc<AtomicBool>);

// Deliberate provider fault injection after successful admission.
unsafe impl<const INDIVISIBLE: bool> NativePayload for TraceFault<INDIVISIBLE> {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = if INDIVISIBLE {
        NativeScanKind::Indivisible
    } else {
        NativeScanKind::Resumable
    };

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut (),
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'a StoredValue),
    ) -> ScanStatus {
        budget.charge(1);
        assert!(!self.0.load(Ordering::Relaxed), "provider trace fault");
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::default()
    }
}

#[test]
fn provider_scan_panic_prevents_any_sweep_or_retry() {
    check_provider_scan_fault::<false>();
    check_provider_scan_fault::<true>();
}

fn check_provider_scan_fault<const INDIVISIBLE: bool>() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let dead = heap
        .alloc_string("unreachable but not swept".into())
        .unwrap();
    let fault = Arc::new(AtomicBool::new(false));
    let object = NativeStorage::payload::<TraceFault<INDIVISIBLE>>()
        .prepare_payload(
            heap,
            &Ty::Builtin(BuiltinType::Unit),
            TraceFault::<INDIVISIBLE>(fault.clone()),
            &owner,
        )
        .unwrap();
    let id = heap.alloc_native(object).unwrap();
    let _root = runtime.root_value(Value::GcHandle(id)).unwrap();
    let before = heap.stats();
    heap.start_collection_observation().unwrap();
    fault.store(true, Ordering::Relaxed);
    assert!(runtime.collect_garbage().is_err());
    assert!(runtime.is_quarantined());
    assert_eq!(heap.stats(), before);
    assert!(heap.validate_value(&dead));
    let observed = heap.collection_observation().unwrap();
    assert_eq!(observed.explicit.count, 1);
    assert_eq!(observed.explicit.failures, 1);
    assert_eq!(observed.cycles.count, 0);
    let scans = observed.indivisible[IndivisibleKind::ProviderScan as usize];
    assert_eq!(scans.count, u64::from(INDIVISIBLE));
    if INDIVISIBLE {
        let scan = scans.slowest.unwrap();
        assert_eq!(scan.rust_type, Some(TypeId::of::<TraceFault<true>>()));
        assert_eq!(scan.work, 1);
        assert!(scan.time <= observed.explicit.total_time);
    }
    fault.store(false, Ordering::Relaxed);
    assert!(runtime.collect_garbage().is_err());
    assert_eq!(heap.stats(), before);
    assert_eq!(
        heap.collection_observation().unwrap(),
        observed,
        "rejection before collector admission is not another service sample"
    );
}

#[test]
fn operation_policy_covers_array_access_without_hiding_type_metadata() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let id = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(10), Value::I32(20)],
        )
        .unwrap();
    let heap = runtime.gc();
    let _root = runtime.root_value(Value::Array(id)).unwrap();
    for kind in [
        NativeOperationKind::ExclusiveSlots,
        NativeOperationKind::ReadOnlyCallbacks,
    ] {
        let guard = heap.try_enter_native_operation(id, kind).unwrap().unwrap();
        assert!(heap.array_contract(id).is_some());
        assert!(heap.validate_value(&Value::Array(id)));
        assert!(heap.array_set(id, 0, Value::I32(99)).is_err());
        assert!(heap.with_buffer_mut(id, 0, |_| ()).is_err());
        if kind == NativeOperationKind::ExclusiveSlots {
            assert_eq!(heap.array_len(id), None);
            assert_eq!(heap.array_get(id, 0), None);
            assert_eq!(heap.array_snapshot(id), None);
            assert!(heap.array_element(id, 0).is_err());
        } else {
            assert_eq!(heap.array_len(id), Some(2));
            assert_eq!(heap.array_get(id, 0), Some(Value::I32(10)));
            assert_eq!(
                heap.array_snapshot(id),
                Some(vec![Value::I32(10), Value::I32(20)])
            );
        }
        drop(guard);
        assert_eq!(
            heap.array_snapshot(id),
            Some(vec![Value::I32(10), Value::I32(20)])
        );
    }
}

#[test]
fn operation_policy_blocks_iterator_receiver_before_preview_or_finish() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let conversion = ConversionContext::new(&runtime, &owner).unwrap();
    let source = Value::Array(
        runtime
            .alloc_array(
                &owner,
                Ty::Builtin(BuiltinType::I32),
                vec![Value::I32(10), Value::I32(20)],
            )
            .unwrap(),
    );
    let _source_root = runtime.root_value(source).unwrap();
    let item_type = conversion.type_for::<i32>().unwrap();
    let source_type = item_type
        .derive(&runtime, &owner, |ty| Some(Ty::Array(Box::new(ty.clone()))))
        .unwrap();
    let heap = runtime.gc();
    let cursor = heap
        .new_iter(&source, &source_type, item_type, &owner)
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
fn partial_native_cursor_keeps_an_edge_moved_behind_it_without_restart() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let child = runtime
        .gc
        .alloc_string("moved behind cursor".into())
        .unwrap();
    let id = allocate(&runtime, &owner, vec![Value::Unit, child]);
    let heap = &runtime.gc;
    heap.graph.lock().begin_cycle().unwrap();
    heap.shade_graph_edge(&runtime.modules, ScanEdge::Value(Value::GcHandle(id)))
        .unwrap();
    heap.graph.lock().finish_roots().unwrap();
    for _ in 0..7 {
        assert_eq!(
            heap.scan_graph_slice(&runtime.modules, &mut ScanBudget::new(1))
                .unwrap(),
            ScanStatus::Pending
        );
    }
    heap.edit_native::<Slots, ()>(
        &runtime,
        id,
        None,
        EditPlan {
            max_logical_units: 2,
            structural: true,
        },
        |payload, access| {
            // SAFETY: The target is still an initialized edge of this exact payload.
            unsafe {
                access.republish_existing(&payload.0[1]);
            }
            access.commit_structure();
            payload.0.swap(0, 1);
            Ok(())
        },
    )
    .unwrap();
    heap.drain_graph_work(&runtime.modules).unwrap();
    heap.graph.lock().close_mark().unwrap();
    assert!(
        heap.graph_survivor(&runtime.modules, child.direct_reference().unwrap().graph())
            .unwrap()
    );
    assert_eq!(
        heap.with_native::<Slots, _>(id, |payload| Ok(StoredValue::values(&payload.0).to_vec()))
            .unwrap(),
        [child, Value::Unit]
    );
    heap.graph.lock().finish_sweep().unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 2);
    assert_eq!(heap.allocated_objects(), 0);
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
                    payload.0.push(StoredValue::scalar(()));
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
            assert_eq!(object.charge, payload.charge());
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
fn structural_revisions_survive_sweep_and_reject_exhaustion_before_edit() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let _garbage = allocate(&runtime, &owner, vec![Value::Unit]);
    let id = allocate(&runtime, &owner, vec![Value::Unit]);
    let _root = runtime.root_value(Value::GcHandle(id)).unwrap();
    let heap = runtime.gc();
    heap.edit_native::<Slots, ()>(
        &runtime,
        id,
        None,
        EditPlan {
            max_logical_units: 2,
            structural: true,
        },
        |payload, access| {
            payload.0.reserve(1);
            access.commit_structure();
            payload.0.push(StoredValue::scalar(()));
            Ok(())
        },
    )
    .unwrap();
    runtime.collect_garbage().unwrap();
    {
        let mut objects = heap.objects.borrow_mut();
        let object = heap.native_mut(&mut objects, id).unwrap();
        assert_eq!(object.revision, 1);
        object.revision = u64::MAX;
    }
    let before = heap.stats();
    let mut entered = false;
    assert!(
        heap.edit_native::<Slots, ()>(
            &runtime,
            id,
            None,
            EditPlan {
                max_logical_units: 3,
                structural: true
            },
            |_, _| {
                entered = true;
                Ok(())
            }
        )
        .is_err()
    );
    assert!(!entered);
    assert_eq!(heap.stats(), before);
    heap.with_native::<Slots, _>(id, |payload| {
        assert_eq!(payload.0.len(), 2);
        Ok(())
    })
    .unwrap();
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
        .read_native::<Slots, _>(
            &runtime,
            ReadReceiver::Value(id),
            None,
            |payload, access| {
                let text = access.inspect(&contract, &payload.0[0], |view| {
                    assert!(runtime.gc().ensure_no_storage_borrow().is_err());
                    Ok(view.as_str()?.to_owned())
                })?;
                assert_eq!(text, "retained");
                assert!(
                    access
                        .inspect(&contract, &StoredValue::scalar(true), |_| Ok(()))
                        .is_err()
                );
                access.retain(&contract, &payload.0[0])
            },
        )
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
                assert!(
                    access
                        .retain(&contract, &StoredValue::scalar(true))
                        .is_err()
                );
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
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 2);
    assert_eq!(runtime.gc().allocated_objects(), 0);
}

#[rstest::rstest]
#[case::termination(false)]
#[case::quarantine(true)]
fn operation_guard_blocks_aliases_and_recovers_after_execution_is_closed(#[case] quarantine: bool) {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let id = allocate(&runtime, &owner, vec![Value::Unit, Value::Unit]);
    let _session = runtime.begin_execution(&owner, Default::default()).unwrap();
    let heap = runtime.gc();
    let retained_bytes = heap.stats().current_retained_bytes;
    let readonly = NativeOperationGuard {
        heap,
        id,
        runtime: Some(&runtime),
        recovery: None,
        _root: Some(runtime.root_value(Value::GcHandle(id)).unwrap()),
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
        _root: Some(runtime.root_value(Value::GcHandle(id)).unwrap()),
        _lease: heap
            .try_enter_native_operation(id, NativeOperationKind::ExclusiveSlots)
            .unwrap()
            .unwrap(),
    };
    assert!(heap.with_native::<Slots, _>(id, |_| Ok(())).is_err());
    assert!(
        heap.read_native::<Slots, _>(&runtime, ReadReceiver::Value(id), None, |_, _| Ok(()))
            .is_err()
    );
    assert!(
        heap.read_native::<Slots, _>(
            &runtime,
            ReadReceiver::Value(id),
            Some(&guard),
            |payload, _| Ok(payload.0.len())
        )
        .is_ok()
    );
    assert!(
        heap.try_enter_native_operation(id, NativeOperationKind::ReadOnlyCallbacks)
            .unwrap()
            .is_none()
    );
    if quarantine {
        runtime
            .resources()
            .quarantine("test closes ordinary execution before cleanup");
    } else {
        heap.resource_limit("test closes ordinary execution before cleanup");
    }
    assert!(heap.ensure_execution_allowed().is_err());
    drop(guard);
    let objects = heap.objects.borrow();
    let object = heap.native_ref(&objects, id).unwrap();
    assert_eq!(object.payload::<Slots>().unwrap().0.len(), 1);
    assert_eq!(object.charge.logical_units, 1);
    assert_eq!(object.revision, 1);
    assert!(!heap.native_operations.is_active(id));
    assert_eq!(heap.stats().current_heap_units, 2);
    assert_eq!(heap.stats().current_retained_bytes, retained_bytes);
}
