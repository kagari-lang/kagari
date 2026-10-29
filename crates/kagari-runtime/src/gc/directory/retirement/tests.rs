use super::*;
use crate::{
    Runtime,
    error::RuntimeError,
    gc::{
        GcHeap,
        directory::{
            PayloadKind,
            pages::{SLAB_BYTES, batch::Disposal},
        },
        reference::GcRef,
    },
    module::ExecutableModule,
    native::{
        payload::data::NativeData,
        storage::{NativeStorage, scan::ScanStatus},
    },
    test_support::layouts::{allocation_owner, interface_value, layout},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    cmp::Reverse,
    panic::{AssertUnwindSafe, catch_unwind},
};

pub(super) fn begin(heap: &GcHeap) {
    let mut graph = heap.graph.lock();
    graph.begin_cycle().unwrap();
    graph.finish_roots().unwrap();
    graph.close_mark().unwrap();
    let counters = heap.resources.counters();
    heap.objects
        .borrow_mut()
        .begin_physical_sweep(
            &graph,
            counters.current_heap_units,
            counters.current_retained_bytes,
        )
        .unwrap();
}

fn begin_with_roots(runtime: &Runtime) {
    let heap = runtime.gc();
    let mut roots = heap
        .begin_initial_roots(&runtime.modules, &runtime.host)
        .unwrap();
    while heap
        .scan_initial_root_slice(
            &runtime.modules,
            &runtime.host,
            &mut roots,
            &mut ScanBudget::new(1024),
        )
        .unwrap()
        == ScanStatus::Pending
    {}
    heap.drain_graph_work(&runtime.modules).unwrap();
    let mut graph = heap.graph.lock();
    graph.close_mark().unwrap();
    let counters = heap.resources.counters();
    heap.objects
        .borrow_mut()
        .begin_physical_sweep(
            &graph,
            counters.current_heap_units,
            counters.current_retained_bytes,
        )
        .unwrap();
}

// The helper supplies the separately precharged workspace allowance; `work`
// controls selection/commit. Driver tests cover the combined 1024-unit limit.
fn try_batch<'a>(heap: &'a GcHeap, budget: &mut ScanBudget) -> Option<SweepBatch<'a>> {
    let mut initialization = ScanBudget::new(WORKSPACE_WORK);
    assert!(initialization.take(WORKSPACE_WORK));
    let mut result = SweepBatch::new(heap);
    heap.objects.borrow_mut().sweep_batch(
        &heap.interfaces,
        &mut heap.graph.lock(),
        budget,
        &mut result,
    )?;
    Some(result)
}

pub(super) fn batch(heap: &GcHeap, work: usize) -> SweepBatch<'_> {
    try_batch(heap, &mut ScanBudget::new(work)).unwrap()
}

/// Fresh heaps allocate monotonically numbered pages; sweep visits newest pages
/// first and occupied slots in ascending order. Keep fixture identities in that
/// order across payload/header size changes, without running the sweep itself.
fn strings_in_sweep_order(heap: &GcHeap, count: usize, capacity: usize) -> Vec<GcRef> {
    let mut ids: Vec<_> = (0..count)
        .map(|_| {
            heap.alloc_string(String::with_capacity(capacity))
                .unwrap()
                .direct_reference()
                .unwrap()
        })
        .collect();
    let objects = heap.objects.borrow();
    ids.sort_by_key(|id| {
        let location = objects.direct_location(*id).unwrap();
        (Reverse(location.page), location.slot)
    });
    ids
}

fn native_scalar(heap: &GcHeap, owner: &ExecutableModule) -> GcRef {
    let payload = NativeStorage::takeable::<i32>()
        .prepare_payload(heap, &Ty::Builtin(BuiltinType::I32), 7, owner)
        .unwrap();
    heap.alloc_native(payload).unwrap()
}

#[test]
fn greedy_page_geometry_fills_32_owners_without_batch_allocations() {
    let mut runtime = Runtime::default();
    let names: Vec<_> = (0..129).map(|index| format!("field{index}")).collect();
    let fields: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), Ty::Builtin(BuiltinType::I64), true))
        .collect();
    let twelve = layout(&mut runtime, "Twelve", &fields[..9]);
    let forty_two = layout(&mut runtime, "FortyTwo", &fields[..2]);
    let large = layout(&mut runtime, "Large", &fields);
    let heap = runtime.gc();
    for _ in 0..504 {
        heap.alloc_struct(twelve.clone(), vec![Value::I64(0); 9])
            .unwrap();
        heap.alloc_struct(forty_two.clone(), vec![Value::I64(0); 2])
            .unwrap();
    }
    for _ in 0..19 {
        heap.alloc_struct(large.clone(), vec![Value::I64(0); 129])
            .unwrap();
    }
    begin(heap);
    let mut counts = [0usize; 33];
    let mut singles = 0;
    let mut transactions = 0;
    let mut observed_objects = 0;
    loop {
        #[cfg(feature = "execution-diagnostics")]
        let mut retired = {
            let (retired, allocations) =
                crate::diagnostics::allocations::measure(|| batch(heap, usize::MAX));
            assert_eq!(allocations.requests, 0, "fixed preparation workspace");
            retired
        };
        #[cfg(not(feature = "execution-diagnostics"))]
        let mut retired = batch(heap, usize::MAX);
        let totals = heap.objects.borrow().sweep.as_ref().unwrap().totals;
        let owners = totals.objects - observed_objects;
        observed_objects = totals.objects;
        assert!(owners <= RETIRE_LIMIT);
        transactions += 1;
        assert_eq!(owners, (1027 - (transactions - 1) * 32).min(32));
        for token in retired.disposal.iter().flatten() {
            match token {
                Disposal::Range(range) => counts[range.objects()] += 1,
                Disposal::Payload(_) => singles += 1,
            }
        }
        // No second batch may detach while this batch retains owners, even when
        // its payloads have no destructor. Charges still include this batch.
        assert!(retired.has_retirements());
        assert!(try_batch(heap, &mut ScanBudget::new(1024)).is_none());
        assert!(heap.objects.borrow_mut().finish_physical_sweep().is_none());
        assert!(heap.resources.counters().current_heap_units >= retired.units);
        let complete = retired.status == ScanStatus::Complete;
        heap.dispose_sweep_batch(&mut retired).unwrap();
        assert!(totals.objects <= 1027);
        if complete {
            break;
        }
    }
    assert_eq!(transactions, 33);
    assert_eq!(
        counts
            .iter()
            .enumerate()
            .map(|(owners, ranges)| owners * ranges)
            .sum::<usize>(),
        1008
    );
    assert_eq!(singles, 19);
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!(totals.objects, 1027);
    assert_eq!(totals.live, 0);
    assert_eq!(heap.objects.borrow().discovery_visits.get(), 1027);
    assert_eq!(heap.objects.borrow().backing_bytes(), 0);
    assert_eq!(heap.resources.counters().current_retained_bytes, 0);
    assert_eq!(heap.resources.counters().current_heap_units, 0);
}

#[test]
fn range_accounting_and_physical_reuse_follow_release_not_detachment() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let ids = strings_in_sweep_order(heap, 42, 7);
    begin(heap);
    let before = heap.resources.counters();
    let locations: Vec<_> = ids
        .iter()
        .map(|id| heap.objects.borrow().direct_location(*id).unwrap())
        .collect();
    let initial_backing = heap.objects.borrow().backing_bytes();
    assert_eq!(
        before.current_retained_bytes,
        initial_backing as u128 + 42 * 7
    );
    let mut retired = batch(heap, 1024);
    assert_eq!(retired.units, 32);
    assert_eq!(retired.bytes, 32 * 7);
    assert_eq!(
        heap.resources.counters().current_heap_units,
        before.current_heap_units
    );
    assert_eq!(
        heap.resources.counters().current_retained_bytes,
        before.current_retained_bytes
    );
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 10);
    assert_eq!(
        heap.dispose_sweep_batch(&mut retired)
            .unwrap()
            .released_bytes,
        initial_backing - SLAB_BYTES,
        "release complete pages while the final ten strings retain one page"
    );
    assert_eq!(heap.resources.counters().current_heap_units, 10);
    assert_eq!(
        heap.resources.counters().current_retained_bytes,
        SLAB_BYTES as u128 + 70
    );

    let Value::Str(newborn) = heap.alloc_string(String::new()).unwrap() else {
        unreachable!()
    };
    let newborn_location = heap.objects.borrow().direct_location(newborn).unwrap();
    assert!(locations[..32].contains(&newborn_location));
    assert!(heap.validate_value(&Value::Str(newborn)));
    let mut retired = batch(heap, 1024);
    assert_eq!(retired.units, 10);
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 1);
    assert_eq!(
        heap.dispose_sweep_batch(&mut retired)
            .unwrap()
            .released_bytes,
        0,
        "newborn pins original page without moving"
    );
    assert_eq!(
        heap.objects
            .borrow_mut()
            .finish_physical_sweep()
            .unwrap()
            .objects,
        42
    );
    assert_eq!(heap.resources.counters().current_heap_units, 1);
    assert_eq!(
        heap.resources.counters().current_retained_bytes,
        SLAB_BYTES as u128
    );
    assert!(heap.validate_value(&Value::Str(newborn)));
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 1);
}

#[derive(Debug, Clone, Copy)]
#[repr(align(64))]
struct Takeable(u64);

// SAFETY: only a fixed scalar, with no script edges or interior state.
unsafe impl NativeData for Takeable {}

#[test]
fn native_take_between_slices_keeps_physical_tombstones_and_newborn_marks() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    for _ in 0..42 {
        heap.alloc_string(String::new()).unwrap();
    }
    let allocate = |number| {
        let object = NativeStorage::takeable::<Takeable>()
            .prepare_payload(
                heap,
                &Ty::Builtin(BuiltinType::Unit),
                Takeable(number),
                &owner,
            )
            .unwrap();
        heap.alloc_native(object).unwrap()
    };
    let old = allocate(17);
    let root = heap.root_value(Value::GcHandle(old)).unwrap();
    let old_location = heap.objects.borrow().direct_location(old).unwrap();
    begin_with_roots(&runtime);
    assert_eq!(heap.take_native::<Takeable>(old).unwrap().0, 17);
    let newborn = allocate(23);
    let new_location = heap.objects.borrow().direct_location(newborn).unwrap();
    assert_eq!(old_location.page, new_location.page);
    assert_ne!(old_location.slot, new_location.slot);
    assert_ne!(old, newborn);
    assert!(heap.take_native::<Takeable>(old).is_err());
    loop {
        let mut retired = batch(heap, 1024);
        let complete = retired.status == ScanStatus::Complete;
        heap.dispose_sweep_batch(&mut retired).unwrap();
        if complete {
            break;
        }
    }
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!(totals.objects, 42);
    assert_eq!(totals.live, 2);
    let retained = heap.objects.borrow().backing_bytes();
    assert_eq!(heap.take_native::<Takeable>(newborn).unwrap().0, 23);
    assert_eq!(heap.objects.borrow().backing_bytes(), retained);
    assert_eq!(
        heap.objects.borrow().direct_location(old).unwrap().kind,
        PayloadKind::Taken
    );
    drop(root);
    heap.graph.lock().finish_sweep().unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 2);
    assert_eq!(heap.objects.borrow().backing_bytes(), 0);
}

#[test]
fn mixed_sparse_pages_reserve_retirement_work_before_detaching() {
    for work_per_slice in [75, 1024] {
        let mut runtime = Runtime::default();
        let shape = layout(
            &mut runtime,
            "Two",
            &[
                ("a", Ty::Builtin(BuiltinType::I64), true),
                ("b", Ty::Builtin(BuiltinType::I64), true),
            ],
        );
        let other = layout(
            &mut runtime,
            "OtherTwo",
            &[
                ("a", Ty::Builtin(BuiltinType::I64), true),
                ("b", Ty::Builtin(BuiltinType::I64), true),
            ],
        );
        let heap = runtime.gc();
        let mut values = Vec::new();
        for _ in 0..6 {
            values.push(Value::Struct(
                heap.alloc_struct(other.clone(), vec![Value::I64(0); 2])
                    .unwrap(),
            ));
            values.push(Value::Struct(
                heap.alloc_struct(shape.clone(), vec![Value::I64(0); 2])
                    .unwrap(),
            ));
        }
        let roots = [values[1], values[8]].map(|value| heap.root_value(value).unwrap());
        let ids: Vec<_> = values
            .iter()
            .map(|value| {
                heap.objects
                    .borrow()
                    .direct_location(value.direct_reference().unwrap())
                    .unwrap()
            })
            .collect();
        let first_page = ids[0].page;
        for id in &ids {
            assert_eq!(id.page, first_page);
        }
        begin_with_roots(&runtime);
        let mut calls = 0;
        let mut work = 0;
        loop {
            let mut budget = ScanBudget::new(work_per_slice);
            let mut retired = try_batch(heap, &mut budget).unwrap();
            assert!(budget.spent() > 0 && budget.spent() <= work_per_slice);
            if work_per_slice == 1024 {
                assert_eq!(
                    (retired.objects, retired.fragments),
                    (10, 10),
                    "one page can contribute disjoint heterogeneous fragments"
                );
            }
            work += budget.spent();
            let complete = retired.status == ScanStatus::Complete;
            heap.dispose_sweep_batch(&mut retired).unwrap();
            calls += 1;
            if complete {
                break;
            }
            assert!(calls < 100, "cursor must progress");
        }
        if work_per_slice == 75 {
            assert!(
                work >= 12
                    + 2 * calls
                    + 10 * (OWNER_RETIRE_WORK + BATCH_FINISH_WORK + FRAGMENT_FINISH_WORK),
                "charge reloaded bitmap planes, occupied slots, and retirement separately"
            );
        } else {
            assert_eq!(calls, 1);
            assert_eq!(
                work,
                2 + 12
                    + 10 * OWNER_RETIRE_WORK
                    + BATCH_FINISH_WORK
                    + 10 * FRAGMENT_FINISH_WORK
                    + ordering_work(10),
                "the aggregate preflight reserves the complete sort-index proof"
            );
        }
        let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
        assert_eq!(totals.objects, 10);
        assert_eq!(totals.live, 2);
        assert_eq!(heap.objects.borrow().backing_bytes(), SLAB_BYTES);
        for (index, id) in ids.into_iter().enumerate() {
            assert_eq!(
                heap.objects.borrow().structure(id).is_some(),
                index == 1 || index == 8
            );
        }
        drop(roots);
    }
}

#[test]
fn bounded_transaction_retires_initialization_owners_and_excludes_newborns() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let mut ids: Vec<_> = (0..42).map(|_| native_scalar(heap, &owner)).collect();
    ids.sort_by_key(|id| {
        let objects = heap.objects.borrow();
        let location = objects.direct_location(*id).unwrap();
        (Reverse(location.page), location.slot)
    });
    let unit = heap.resources.counters().current_heap_units / 42;
    {
        let mut objects = heap.objects.borrow_mut();
        for &id in &ids {
            objects.initialization_owners.insert(id, owner.key());
        }
    }
    begin(heap);
    let mut retired = batch(heap, usize::MAX);
    {
        let objects = heap.objects.borrow();
        assert_eq!(objects.initialization_owners.len(), 10);
        for (ordinal, id) in ids.iter().enumerate() {
            assert_eq!(
                objects.initialization_owners.contains_key(id),
                ordinal >= 32
            );
        }
    }
    heap.dispose_sweep_batch(&mut retired).unwrap();
    for _ in 0..32 {
        native_scalar(heap, &owner);
    }
    let mut retired = batch(heap, usize::MAX);
    assert_eq!(retired.units, 10 * unit);
    heap.dispose_sweep_batch(&mut retired).unwrap();
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!(totals.objects, 42);
    assert_eq!(totals.live, 32);
    assert!(heap.objects.borrow().initialization_owners.is_empty());
    assert_eq!(heap.resources.counters().current_heap_units, 32 * unit);
}

#[test]
fn borrowed_retirement_proof_cannot_replay_into_outstanding_or_reused_storage() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let old = heap.alloc_string(String::with_capacity(7)).unwrap();
    let id = old.direct_reference().unwrap();
    let old_location = heap.objects.borrow().direct_location(id).unwrap();
    begin(heap);
    let mut initialization = ScanBudget::new(WORKSPACE_WORK);
    assert!(initialization.take(WORKSPACE_WORK));
    let mut retired = SweepBatch::new(heap);
    let mut prepared = Prepared::new();
    {
        let mut objects = heap.objects.borrow_mut();
        let mut selection = Selection::new();
        let mut marker = heap.graph.lock();
        let graph = marker.sweep_batch().unwrap();
        let physical = objects
            .pages
            .prepare_batch(
                &mut selection,
                &mut retired.disposal,
                &mut ScanBudget::new(usize::MAX),
                |slot, _, _| {
                    assert!(
                        !graph
                            .is_survivor(slot.marks, slot.location.slot as usize)
                            .unwrap()
                    );
                    let charge = slot
                        .payload
                        .view(&heap.interfaces)?
                        .charge(&heap.interfaces)?;
                    prepared.push(
                        slot.payload.direct_reference(),
                        charge.logical_units,
                        charge.retained_bytes,
                    )?;
                    Some(Decision::Retire)
                },
            )
            .unwrap();
        let totals = SweepTotals {
            objects: 1,
            units: prepared.units,
            bytes: prepared.bytes,
            live: 0,
        };
        let credits = graph.prepare_retirement(1).unwrap();
        let proof = prepared.seal();
        retired.fragments = physical.fragments();
        physical.detach();
        proof.commit(&mut objects, &mut retired, totals);
        credits.commit();
    }
    let before = heap.resources.counters();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = prepared.seal();
        }))
        .is_err()
    );
    {
        let objects = heap.objects.borrow();
        assert!(objects.sweep.as_ref().unwrap().outstanding);
        assert!(objects.pages.get(old_location).is_none());
    }
    assert_eq!(heap.resources.counters(), before);
    heap.dispose_sweep_batch(&mut retired).unwrap();
    let newborn = heap.alloc_string(String::with_capacity(11)).unwrap();
    let new_id = newborn.direct_reference().unwrap();
    assert_eq!(
        heap.objects.borrow().direct_location(new_id).unwrap(),
        old_location
    );
    let before = heap.resources.counters();
    let location = heap.objects.borrow().direct_location(new_id);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = prepared.seal();
        }))
        .is_err()
    );
    {
        let objects = heap.objects.borrow();
        assert!(!objects.sweep.as_ref().unwrap().outstanding);
        assert_eq!(objects.direct_location(new_id), location);
    }
    assert!(heap.validate_value(&newborn));
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 1);
    assert_eq!(heap.resources.counters(), before);
    assert_eq!(
        heap.objects
            .borrow_mut()
            .finish_physical_sweep()
            .unwrap()
            .live,
        1
    );
}

#[test]
fn bounded_transaction_rejects_late_credit_underflow_before_detachment() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let ids = strings_in_sweep_order(heap, 42, 17);
    begin(heap);
    // Deliberately corrupt the credit ledger by consuming one unretired credit.
    // The first physical batch can commit; the second must fail atomically.
    heap.graph
        .lock()
        .sweep_batch()
        .unwrap()
        .prepare_retirement(1)
        .unwrap()
        .commit();
    let mut retired = batch(heap, 1024);
    assert_eq!(retired.units, 32);
    heap.dispose_sweep_batch(&mut retired).unwrap();
    let before = heap.resources.counters();
    let locations: Vec<_> = ids[32..]
        .iter()
        .map(|id| heap.objects.borrow().direct_location(*id).unwrap())
        .collect();
    let error = heap
        .retire_physical_batch(&mut ScanBudget::new(1024))
        .err()
        .expect("late credit underflow rejects the transaction");
    assert_eq!(
        error,
        RuntimeError::module_validation("invalid physical retirement transaction")
    );
    {
        let mut graph = heap.graph.lock();
        // Failure consumed neither the remaining nine credits nor any of the ten
        // payload owners. Capacity/invariant checking belongs to the graph owner.
        let _uncommitted = graph.sweep_batch().unwrap().prepare_retirement(9).unwrap();
    }
    for (id, location) in ids[32..].iter().zip(locations) {
        let objects = heap.objects.borrow();
        assert_eq!(objects.direct_location(*id), Some(location));
        assert!(objects.pages.get(location).is_some());
    }
    assert_eq!(
        heap.resources.counters().current_heap_units,
        before.current_heap_units
    );
    assert_eq!(
        heap.resources.counters().current_retained_bytes,
        before.current_retained_bytes
    );
    assert_eq!(before.current_heap_units, 10);
    assert_eq!(before.current_retained_bytes, SLAB_BYTES as u128 + 170);
    heap.graph.lock().fault();
    assert!(try_batch(heap, &mut ScanBudget::new(1024)).is_none());
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 10);
}

#[test]
fn retirement_reserves_disposal_and_finish_before_detaching() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let value = heap.alloc_string(String::with_capacity(7)).unwrap();
    begin(heap);
    let before = heap.resources.counters();
    for (work, inspected) in [(1, 0), (2, 2), (3, 3), (74, 3)] {
        let mut budget = ScanBudget::new(work);
        let mut retired = try_batch(heap, &mut budget).unwrap();
        assert_eq!(retired.status, ScanStatus::Pending);
        assert_eq!(retired.objects, 0);
        assert!(!retired.has_retirements());
        assert!(retired.disposal.iter().all(Option::is_none));
        assert!(!heap.objects.borrow().sweep.as_ref().unwrap().outstanding);
        assert_eq!(
            budget.spent(),
            inspected,
            "failed reservation leaves the owner in place"
        );
        assert!(heap.validate_value(&value));
        let released = heap.dispose_sweep_batch(&mut retired).unwrap();
        assert_eq!(
            (released.objects, released.units, released.bytes),
            (0, 0, 0)
        );
        assert_eq!(
            heap.resources.counters().current_retained_bytes,
            before.current_retained_bytes
        );
    }
    let mut budget = ScanBudget::new(75);
    let mut retired = try_batch(heap, &mut budget).unwrap();
    assert_eq!(retired.objects, 1);
    assert_eq!(budget.spent(), 75);
    assert!(!heap.validate_value(&value));
    assert_eq!(heap.resources.counters().current_heap_units, 1);
    let released = heap.dispose_sweep_batch(&mut retired).unwrap();
    assert_eq!(
        (released.objects, released.units, released.bytes),
        (1, 1, SLAB_BYTES as u128 + 7)
    );
    assert_eq!(
        budget.spent(),
        75,
        "disposal and finish were already reserved"
    );
    assert_eq!(heap.resources.counters().current_heap_units, 0);
    loop {
        let mut retired = batch(heap, 1);
        let status = retired.status;
        assert_eq!(retired.objects, 0);
        heap.dispose_sweep_batch(&mut retired).unwrap();
        if status == ScanStatus::Complete {
            break;
        }
    }
    assert_eq!(
        heap.objects
            .borrow_mut()
            .finish_physical_sweep()
            .unwrap()
            .objects,
        1
    );
}

#[test]
fn inspection_progress_cannot_finish_another_batch_and_reports_newborn_liveness() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let _old = heap.alloc_string(String::with_capacity(7)).unwrap();
    begin(heap);
    let mut progress = batch(heap, 1);
    assert!(!progress.has_retirements());
    assert_eq!(progress.status, ScanStatus::Pending);
    assert!(!heap.objects.borrow().sweep.as_ref().unwrap().outstanding);
    let mut retired = batch(heap, 75);
    assert!(retired.has_retirements());
    let before = heap.resources.counters();
    {
        let objects = heap.objects.borrow_mut();
        let _stats = heap.stats.borrow_mut();
        let empty = heap.dispose_sweep_batch(&mut progress).unwrap();
        assert_eq!((empty.objects, empty.units, empty.bytes), (0, 0, 0));
        assert!(objects.sweep.as_ref().unwrap().outstanding);
        assert_eq!(heap.resources.counters(), before);
    }
    assert!(heap.objects.borrow_mut().finish_physical_sweep().is_none());
    heap.dispose_sweep_batch(&mut retired).unwrap();
    let newborn = heap.alloc_string(String::new()).unwrap();
    let complete = batch(heap, 0);
    assert_eq!(complete.status, ScanStatus::Complete);
    assert!(!complete.has_retirements());
    // Progress-only completion has no ownership requiring a disposal handshake.
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!((totals.objects, totals.live), (1, 1));
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 1);
    assert!(heap.validate_value(&newborn));
    assert_eq!(heap.resources.counters().current_heap_units, 1);
}

#[test]
fn interface_page_cursor_excludes_reused_newborns_and_retains_metadata() {
    let mut runtime = Runtime::default();
    let Value::Interface(original) = interface_value(&mut runtime) else {
        unreachable!()
    };
    let heap = runtime.gc();
    let snapshot = heap.interface_snapshot_id(original).unwrap();
    let mut ids = vec![original];
    for _ in 0..6 {
        ids.push(heap.alloc_interface(snapshot).unwrap());
    }
    let _roots = [
        heap.root_value(Value::Interface(ids[0])).unwrap(),
        heap.root_value(Value::Interface(ids[4])).unwrap(),
    ];
    let reused_location = heap.objects.borrow().direct_location(ids[1].0).unwrap();
    begin_with_roots(&runtime);
    // Two bitmap planes, one survivor and one dead owner, plus the complete
    // physical fragment/transaction reservation. Admit exactly one dead wrapper.
    let one_owner = 2 + 1 + OWNER_RETIRE_WORK + BATCH_FINISH_WORK + FRAGMENT_FINISH_WORK;
    let mut first = batch(heap, one_owner + 1);
    assert_eq!(first.objects, 1);
    assert!(
        first.disposal.iter().any(Option::is_some),
        "interface wrapper owns physical retirement"
    );
    assert!(
        first.has_retirements(),
        "wrapper still owns committed retirement"
    );
    assert!(heap.objects.borrow().sweep.as_ref().unwrap().outstanding);
    assert_eq!(first.status, ScanStatus::Pending);
    heap.dispose_sweep_batch(&mut first).unwrap();
    assert!(heap.objects.borrow().pages.get(reused_location).is_none());
    let newborn = heap.alloc_interface(snapshot).unwrap();
    let mut second = batch(heap, one_owner);
    assert_eq!(second.objects, 1);
    heap.dispose_sweep_batch(&mut second).unwrap();
    assert_eq!(
        heap.objects.borrow().direct_location(newborn.0).unwrap(),
        reused_location
    );
    loop {
        let mut retired = batch(heap, one_owner);
        assert!(retired.objects <= 1);
        let status = retired.status;
        heap.dispose_sweep_batch(&mut retired).unwrap();
        assert!(heap.validate_value(&Value::Interface(newborn)));
        assert!(heap.interface_metadata(snapshot).is_some());
        if status == ScanStatus::Complete {
            break;
        }
    }
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!((totals.objects, totals.live), (5, 3));
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 3);
    for id in [ids[0], ids[4]] {
        assert!(heap.validate_value(&Value::Interface(id)));
    }
}

#[test]
fn zero_work_and_failed_accounting_preflight_cannot_detach() {
    for invalid in [false, true] {
        let runtime = Runtime::default();
        let heap = runtime.gc();
        let Value::Str(id) = heap.alloc_string(String::with_capacity(17)).unwrap() else {
            unreachable!()
        };
        begin(heap);
        let mut empty = batch(heap, 0);
        assert!(empty.status != ScanStatus::Complete);
        assert!(empty.disposal.iter().all(Option::is_none));
        assert_eq!(
            heap.dispose_sweep_batch(&mut empty).unwrap().released_bytes,
            0
        );
        let location = heap.objects.borrow().direct_location(id).unwrap();
        if invalid {
            heap.objects.borrow_mut().sweep.as_mut().unwrap().max_bytes = 16;
        } else {
            heap.objects
                .borrow_mut()
                .sweep
                .as_mut()
                .unwrap()
                .totals
                .units = usize::MAX;
        }
        let before = heap.resources.counters();
        assert!(try_batch(heap, &mut ScanBudget::new(1024)).is_none());
        let objects = heap.objects.borrow();
        assert_eq!(objects.direct_location(id), Some(location));
        assert!(objects.pages.get(location).is_some());
        assert_eq!(objects.backing_bytes(), SLAB_BYTES);
        assert_eq!(
            heap.resources.counters().current_heap_units,
            before.current_heap_units
        );
        assert_eq!(
            heap.resources.counters().current_retained_bytes,
            before.current_retained_bytes
        );
    }
}
