use super::*;
use crate::{
    Runtime,
    gc::{
        GcHeap,
        directory::{
            maintenance::MaintenanceObservation,
            pages::{SLAB_BYTES, batch::Disposal},
        },
        metrics::IndivisibleKind,
    },
    native::{
        payload::data::NativeData,
        storage::{NativeStorage, scan::ScanStatus},
    },
    test_support::layouts::{allocation_owner, interface_value, layout},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    mem::size_of_val,
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
        heap.owner,
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

#[test]
fn greedy_page_geometry_fills_32_owners_without_batch_allocations() {
    let mut runtime = Runtime::default();
    let names: Vec<_> = (0..129).map(|index| format!("field{index}")).collect();
    let fields: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), Ty::Builtin(BuiltinType::I64), true))
        .collect();
    let twelve = layout(&mut runtime, "Twelve", &fields[..10]);
    let forty_two = layout(&mut runtime, "FortyTwo", &fields[..3]);
    let large = layout(&mut runtime, "Large", &fields);
    let heap = runtime.gc();
    for _ in 0..504 {
        heap.alloc_struct(twelve.clone(), vec![Value::I64(0); 10])
            .unwrap();
        heap.alloc_struct(forty_two.clone(), vec![Value::I64(0); 3])
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
        heap.dispose_sweep_batch(&mut retired);
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
fn range_accounting_and_generation_reuse_follow_release_not_detachment() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let mut ids = Vec::new();
    for _ in 0..42 {
        let Value::Str(id) = heap.alloc_string(String::with_capacity(7)).unwrap() else {
            unreachable!()
        };
        ids.push(id);
    }
    begin(heap);
    let before = heap.resources.counters();
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
    assert_eq!(heap.dispose_sweep_batch(&mut retired).released_bytes, 0);
    assert_eq!(heap.resources.counters().current_heap_units, 10);
    assert_eq!(
        heap.resources.counters().current_retained_bytes,
        SLAB_BYTES as u128 + 70
    );

    let Value::Str(newborn) = heap.alloc_string(String::new()).unwrap() else {
        unreachable!()
    };
    assert!(
        ids[..32]
            .iter()
            .any(|old| old.index() == newborn.index() && old.generation != newborn.generation)
    );
    assert!(heap.validate_value(&Value::Str(newborn)));
    let mut retired = batch(heap, 1024);
    assert_eq!(retired.units, 10);
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 1);
    assert_eq!(
        heap.dispose_sweep_batch(&mut retired).released_bytes,
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
    assert!(ids.iter().all(|&id| !heap.validate_value(&Value::Str(id))));
}

#[derive(Debug, Clone, Copy)]
#[repr(align(64))]
struct Takeable(u64);

// SAFETY: only a fixed scalar, with no script edges or interior state.
unsafe impl NativeData for Takeable {}

#[test]
fn native_take_between_slices_repairs_cursor_before_directory_and_page_reuse() {
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
    let old_location = heap.objects.borrow().slots[old.index()].location().unwrap();
    begin_with_roots(&runtime);
    assert_eq!(heap.take_native::<Takeable>(old).unwrap().0, 17);
    let newborn = allocate(23);
    let new_location = heap.objects.borrow().slots[newborn.index()]
        .location()
        .unwrap();
    assert_eq!(old_location.page, new_location.page);
    assert_eq!(old.index(), newborn.index());
    assert_ne!(old.generation, newborn.generation);
    assert!(heap.take_native::<Takeable>(old).is_err());
    loop {
        let mut retired = batch(heap, 1024);
        let complete = retired.status == ScanStatus::Complete;
        heap.dispose_sweep_batch(&mut retired);
        if complete {
            break;
        }
    }
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!(totals.objects, 42);
    assert_eq!(totals.live, 1);
    assert_eq!(heap.take_native::<Takeable>(newborn).unwrap().0, 23);
    assert_eq!(heap.objects.borrow().backing_bytes(), 0);
    assert!(heap.moved.borrow().contains_key(&old));
    drop(root);
}

#[test]
fn mixed_sparse_pages_reserve_retirement_work_before_detaching() {
    for work_per_slice in [75, 1024] {
        let mut runtime = Runtime::default();
        let shape = layout(
            &mut runtime,
            "Three",
            &[
                ("a", Ty::Builtin(BuiltinType::I64), true),
                ("b", Ty::Builtin(BuiltinType::I64), true),
                ("c", Ty::Builtin(BuiltinType::I64), true),
            ],
        );
        let heap = runtime.gc();
        let mut values = Vec::new();
        for _ in 0..6 {
            values.push(heap.alloc_string("payload".into()).unwrap());
            values.push(Value::Struct(
                heap.alloc_struct(shape.clone(), vec![Value::I64(0); 3])
                    .unwrap(),
            ));
        }
        let roots = [values[1], values[8]].map(|value| heap.root_value(value).unwrap());
        let first_page = heap.objects.borrow().slots[values[0].object_id().unwrap().index()]
            .location()
            .unwrap()
            .page;
        for value in &values {
            assert_eq!(
                heap.objects.borrow().slots[value.object_id().unwrap().index()]
                    .location()
                    .unwrap()
                    .page,
                first_page
            );
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
            heap.dispose_sweep_batch(&mut retired);
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
        for (index, value) in values.into_iter().enumerate() {
            assert_eq!(heap.validate_value(&value), index == 1 || index == 8);
        }
        drop(roots);
    }
}

#[test]
fn bounded_transaction_preserves_initialization_owners_and_exhausted_generations() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let mut ids: Vec<_> = (0..42)
        .map(|_| {
            heap.alloc_string(String::new())
                .unwrap()
                .object_id()
                .unwrap()
        })
        .collect();
    {
        let mut objects = heap.objects.borrow_mut();
        for ordinal in [0, 17] {
            let index = ids[ordinal].index();
            objects.slots[index].generation = u32::MAX;
            ids[ordinal] = HeapObjectId::new(heap.owner, index, u32::MAX);
        }
        for &id in &ids {
            objects.initialization_owners.insert(id, owner.key());
        }
    }
    begin(heap);
    let mut retired = batch(heap, 1024);
    {
        let objects = heap.objects.borrow();
        assert_eq!(objects.initialization_owners.len(), 10);
        for (ordinal, id) in ids.iter().enumerate() {
            assert_eq!(
                objects.initialization_owners.contains_key(id),
                ordinal >= 32
            );
            let slot = &objects.slots[id.index()];
            if ordinal < 32 {
                assert!(slot.location().is_none());
                assert_eq!(slot.generation, id.generation.saturating_add(1));
            } else {
                assert!(slot.location().is_some());
                assert_eq!(slot.generation, id.generation);
            }
        }
    }
    heap.dispose_sweep_batch(&mut retired);
    for _ in 0..32 {
        let new = heap
            .alloc_string(String::new())
            .unwrap()
            .object_id()
            .unwrap();
        assert_ne!(new.index(), ids[0].index());
        assert_ne!(new.index(), ids[17].index());
        assert!(!ids.contains(&new), "full identities never alias");
    }
    let mut retired = batch(heap, 1024);
    assert_eq!(retired.units, 10);
    heap.dispose_sweep_batch(&mut retired);
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!(totals.objects, 42);
    assert_eq!(totals.live, 32);
    assert!(heap.objects.borrow().initialization_owners.is_empty());
    assert_eq!(heap.resources.counters().current_heap_units, 32);
}

#[test]
fn borrowed_retirement_proof_cannot_replay_into_outstanding_or_reused_storage() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let old = heap.alloc_string(String::with_capacity(7)).unwrap();
    let id = old.object_id().unwrap();
    begin(heap);
    let mut initialization = ScanBudget::new(WORKSPACE_WORK);
    assert!(initialization.take(WORKSPACE_WORK));
    let mut retired = SweepBatch::new(heap);
    let mut prepared = Prepared::new(heap.owner);
    {
        let mut objects = heap.objects.borrow_mut();
        let mut scanner = objects.pages.scanner();
        let physical = scanner
            .peek(&mut ScanBudget::new(usize::MAX))
            .unwrap()
            .unwrap();
        scanner.advance();
        let cursor = scanner.cursor();
        let candidate = Candidate {
            index: id.index(),
            location: physical.location,
            physical: Some((physical.descriptor, physical.small)),
        };
        let mut marker = heap.graph.lock();
        let graph = marker.sweep_batch().unwrap();
        assert!(matches!(
            objects.prepare_owner(
                &candidate,
                &heap.interfaces,
                &graph,
                &mut prepared,
                &mut ScanBudget::new(usize::MAX)
            ),
            Some(PreparedOwner::Retire)
        ));
        let totals = SweepTotals {
            objects: 1,
            units: prepared.units,
            bytes: prepared.bytes,
            live: 0,
        };
        let credits = graph.prepare_retirement(1).unwrap();
        let proof = prepared.seal();
        let checked = objects
            .pages
            .prepare_batch(proof.fragments, &mut retired.disposal)
            .unwrap();
        retired.fragments = proof.fragments.len();
        checked.detach();
        objects.pages.publish_sweep(cursor);
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
        assert_eq!(objects.slots[id.index()].generation, id.generation + 1);
        assert!(objects.slots[id.index()].location().is_none());
    }
    assert_eq!(heap.resources.counters(), before);
    heap.dispose_sweep_batch(&mut retired);
    let newborn = heap.alloc_string(String::with_capacity(11)).unwrap();
    let new_id = newborn.object_id().unwrap();
    assert_eq!(new_id.index(), id.index());
    assert_ne!(new_id, id);
    let before = heap.resources.counters();
    let location = heap.objects.borrow().slots[new_id.index()].location();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = prepared.seal();
        }))
        .is_err()
    );
    {
        let objects = heap.objects.borrow();
        assert!(!objects.sweep.as_ref().unwrap().outstanding);
        assert_eq!(objects.slots[new_id.index()].location(), location);
        assert_eq!(objects.slots[new_id.index()].generation, new_id.generation);
    }
    assert!(heap.validate_value(&newborn));
    assert!(!heap.validate_value(&old));
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
    let ids: Vec<_> = (0..42)
        .map(|_| {
            heap.alloc_string(String::with_capacity(17))
                .unwrap()
                .object_id()
                .unwrap()
        })
        .collect();
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
    heap.dispose_sweep_batch(&mut retired);
    let before = heap.resources.counters();
    let locations: Vec<_> = ids[32..]
        .iter()
        .map(|id| heap.objects.borrow().slots[id.index()].location().unwrap())
        .collect();
    assert!(try_batch(heap, &mut ScanBudget::new(1024)).is_none());
    {
        let mut graph = heap.graph.lock();
        // Failure consumed neither the remaining nine credits nor any of the ten
        // payload owners. Capacity/invariant checking belongs to the graph owner.
        let _uncommitted = graph.sweep_batch().unwrap().prepare_retirement(9).unwrap();
    }
    for (id, location) in ids[32..].iter().zip(locations) {
        let objects = heap.objects.borrow();
        assert_eq!(objects.slots[id.index()].generation, id.generation);
        assert_eq!(objects.slots[id.index()].location(), Some(location));
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
        let released = heap.dispose_sweep_batch(&mut retired);
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
    let released = heap.dispose_sweep_batch(&mut retired);
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
        heap.dispose_sweep_batch(&mut retired);
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
    let old = heap.alloc_string(String::with_capacity(7)).unwrap();
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
        let empty = heap.dispose_sweep_batch(&mut progress);
        assert_eq!((empty.objects, empty.units, empty.bytes), (0, 0, 0));
        assert!(objects.sweep.as_ref().unwrap().outstanding);
        assert_eq!(heap.resources.counters(), before);
    }
    assert!(heap.objects.borrow_mut().finish_physical_sweep().is_none());
    heap.dispose_sweep_batch(&mut retired);
    let newborn = heap.alloc_string(String::new()).unwrap();
    let complete = batch(heap, 0);
    assert_eq!(complete.status, ScanStatus::Complete);
    assert!(!complete.has_retirements());
    // Progress-only completion has no ownership requiring a disposal handshake.
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!((totals.objects, totals.live), (1, 1));
    assert!(!heap.validate_value(&old));
    assert!(heap.validate_value(&newborn));
    assert_eq!(heap.resources.counters().current_heap_units, 1);
}

#[test]
fn reverse_wrapper_cursor_repairs_swaps_and_excludes_reused_newborns() {
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
    let roots = [
        heap.root_value(Value::Interface(ids[0])).unwrap(),
        heap.root_value(Value::Interface(ids[4])).unwrap(),
    ];
    begin_with_roots(&runtime);
    let mut first = batch(heap, 41);
    assert_eq!(first.objects, 1);
    assert!(
        first.disposal.iter().all(Option::is_none),
        "wrapper has no physical drop token"
    );
    assert!(
        first.has_retirements(),
        "wrapper still owns committed retirement"
    );
    assert!(heap.objects.borrow().sweep.as_ref().unwrap().outstanding);
    assert_eq!(first.status, ScanStatus::Pending);
    heap.dispose_sweep_batch(&mut first);
    assert!(!heap.validate_value(&Value::Interface(ids[6])));
    let newborn = heap.alloc_interface(snapshot).unwrap();
    assert_eq!(newborn.0.index(), ids[6].0.index());
    assert_ne!(newborn.0.generation, ids[6].0.generation);
    let mut second = batch(heap, 41);
    assert_eq!(second.objects, 1);
    heap.dispose_sweep_batch(&mut second);
    assert_eq!(
        heap.objects.borrow().slots[newborn.0.index()]
            .location()
            .unwrap()
            .page,
        5
    );
    loop {
        let mut retired = batch(heap, 42);
        assert!(retired.objects <= 1);
        let status = retired.status;
        heap.dispose_sweep_batch(&mut retired);
        let objects = heap.objects.borrow();
        assert!(
            (0..objects.interface_directories.len()).all(|ordinal| objects.valid_wrapper(ordinal))
        );
        assert!(heap.validate_value(&Value::Interface(newborn)));
        if status == ScanStatus::Complete {
            break;
        }
    }
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!((totals.objects, totals.live), (5, 3));
    assert_eq!(heap.objects.borrow().interface_directories.len(), 3);
    for (ordinal, id) in ids.iter().enumerate() {
        assert_eq!(
            heap.validate_value(&Value::Interface(*id)),
            ordinal == 0 || ordinal == 4
        );
    }
    assert!(roots.iter().all(|root| root.value(heap).is_some()));
}

#[test]
fn maintenance_charges_one_off_compaction_without_discarding_generation_history() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let values: Vec<_> = (0..17)
        .map(|_| heap.alloc_string(String::new()).unwrap())
        .collect();
    begin(heap);
    loop {
        let mut retired = batch(heap, 1024);
        let status = retired.status;
        heap.dispose_sweep_batch(&mut retired);
        if status == ScanStatus::Complete {
            break;
        }
    }
    heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    heap.start_collection_observation().unwrap();
    heap.stats
        .borrow_mut()
        .observation
        .as_mut()
        .unwrap()
        .collecting = true;
    let mut observation = MaintenanceObservation::new(heap.observation_clock());
    let mut objects = heap.objects.borrow_mut();
    let history = objects.slots.len();
    assert!(objects.slots.capacity() > history);
    let mut maintenance = objects.begin_maintenance();
    let mut descriptor_work = ScanBudget::new(4);
    let step = objects.maintenance_slice(
        &mut maintenance,
        &mut descriptor_work,
        Some(&mut observation),
    );
    assert_eq!(observation.destruction.count, 1);
    assert!(observation.allocation.is_none());
    assert_eq!(step.status, ScanStatus::Pending);
    assert!(step.removed_structure.is_none());
    assert_eq!(descriptor_work.spent(), 4);
    let mut shrink_work = ScanBudget::new(1);
    let step =
        objects.maintenance_slice(&mut maintenance, &mut shrink_work, Some(&mut observation));
    let (kind, sample) = observation.allocation.unwrap();
    assert_eq!(kind, IndivisibleKind::DirectoryCompaction);
    assert_eq!(sample.items, history);
    assert_eq!(sample.work, history);
    assert_eq!(
        sample.known_bytes,
        Some(size_of_val(objects.slots.as_slice()))
    );
    assert_eq!(
        step.status,
        ScanStatus::Pending,
        "stop after the one-off allocation operation"
    );
    assert_eq!(
        shrink_work.spent(),
        history + 1,
        "explicit indivisible copy charge"
    );
    assert_eq!(objects.slots.len(), history);
    assert_eq!(objects.slots.capacity(), history);
    while objects
        .maintenance_slice(
            &mut maintenance,
            &mut ScanBudget::new(1),
            Default::default(),
        )
        .status
        != ScanStatus::Complete
    {}
    drop(objects);
    assert!(values.iter().all(|value| !heap.validate_value(value)));
    let Value::Str(newborn) = heap.alloc_string(String::new()).unwrap() else {
        unreachable!()
    };
    assert_eq!(heap.objects.borrow().slots.len(), history);
    assert_eq!(newborn.generation, 1);
    assert!(values.iter().all(|value| !heap.validate_value(value)));
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
        assert_eq!(heap.dispose_sweep_batch(&mut empty).released_bytes, 0);
        let location = heap.objects.borrow().slots[id.index()].location().unwrap();
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
        assert_eq!(objects.slots[id.index()].generation, id.generation);
        assert_eq!(objects.slots[id.index()].location(), Some(location));
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
