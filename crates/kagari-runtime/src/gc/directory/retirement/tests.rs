use super::*;
use crate::{
    Runtime,
    gc::directory::pages::SLAB_BYTES,
    native::{
        payload::data::NativeData,
        storage::{NativeStorage, scan::ScanStatus},
    },
    test_support::layouts::{allocation_owner, layout},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn begin(heap: &GcHeap) {
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

fn batch(heap: &GcHeap, work: usize) -> SweepBatch {
    heap.objects
        .borrow_mut()
        .sweep_batch(
            heap.owner,
            &heap.interfaces,
            &mut heap.graph.lock(),
            &mut ScanBudget::new(work),
        )
        .unwrap()
}

#[test]
fn page_geometry_uses_66_ranges_and_19_individual_tokens_without_batch_allocations() {
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
    let mut observed_objects = 0;
    loop {
        #[cfg(feature = "execution-diagnostics")]
        let retired = {
            let (retired, allocations) =
                crate::diagnostics::allocations::measure(|| batch(heap, 1024));
            assert_eq!(allocations.requests, 0, "fixed preparation workspace");
            retired
        };
        #[cfg(not(feature = "execution-diagnostics"))]
        let retired = batch(heap, 1024);
        let totals = heap.objects.borrow().sweep.as_ref().unwrap().totals;
        let owners = totals.objects - observed_objects;
        observed_objects = totals.objects;
        assert!(owners <= RETIRE_LIMIT);
        match &retired.disposal {
            Some(Disposal::Range(_)) => {
                counts[owners] += 1;
            }
            Some(Disposal::Payload(_)) => singles += 1,
            None => {}
        }
        // No second batch may detach while this batch retains owners, even when
        // its payloads have no destructor. Charges still include this batch.
        assert!(
            heap.objects
                .borrow_mut()
                .sweep_batch(
                    heap.owner,
                    &heap.interfaces,
                    &mut heap.graph.lock(),
                    &mut ScanBudget::new(1024),
                )
                .is_none()
        );
        assert!(heap.objects.borrow_mut().finish_physical_sweep().is_none());
        assert!(heap.resources.counters().current_heap_units >= retired.units);
        let complete = retired.complete;
        heap.dispose_sweep_batch(retired);
        assert!(totals.objects <= 1027);
        if complete {
            break;
        }
    }
    assert_eq!(counts[12], 42);
    assert_eq!(counts[32], 12);
    assert_eq!(counts[10], 12);
    assert_eq!(counts.iter().sum::<usize>(), 66);
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
    let retired = batch(heap, 1024);
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
    assert_eq!(heap.dispose_sweep_batch(retired), 0);
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
    let retired = batch(heap, 1024);
    assert_eq!(retired.units, 10);
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 1);
    assert_eq!(
        heap.dispose_sweep_batch(retired),
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
        let retired = batch(heap, 1024);
        let complete = retired.complete;
        heap.dispose_sweep_batch(retired);
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
fn mixed_sparse_pages_and_survivors_progress_with_one_slot_budget() {
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
    loop {
        let mut budget = ScanBudget::new(1);
        let retired = heap
            .objects
            .borrow_mut()
            .sweep_batch(
                heap.owner,
                &heap.interfaces,
                &mut heap.graph.lock(),
                &mut budget,
            )
            .unwrap();
        assert_eq!(budget.spent(), 1, "holes and survivors also cost work");
        let complete = retired.complete;
        heap.dispose_sweep_batch(retired);
        calls += 1;
        if complete {
            break;
        }
        assert!(calls < 100, "cursor must progress");
    }
    assert_eq!(calls, 42);
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!(totals.objects, 10);
    assert_eq!(totals.live, 2);
    assert_eq!(heap.objects.borrow().backing_bytes(), SLAB_BYTES);
    for (index, value) in values.into_iter().enumerate() {
        assert_eq!(heap.validate_value(&value), index == 1 || index == 8);
    }
    drop(roots);
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
    let retired = batch(heap, 1024);
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
    heap.dispose_sweep_batch(retired);
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
    let retired = batch(heap, 1024);
    assert_eq!(retired.units, 10);
    heap.dispose_sweep_batch(retired);
    let totals = heap.objects.borrow_mut().finish_physical_sweep().unwrap();
    assert_eq!(totals.objects, 42);
    assert_eq!(totals.live, 32);
    assert!(heap.objects.borrow().initialization_owners.is_empty());
    assert_eq!(heap.resources.counters().current_heap_units, 32);
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
    let retired = batch(heap, 1024);
    assert_eq!(retired.units, 32);
    heap.dispose_sweep_batch(retired);
    let before = heap.resources.counters();
    let locations: Vec<_> = ids[32..]
        .iter()
        .map(|id| heap.objects.borrow().slots[id.index()].location().unwrap())
        .collect();
    assert!(
        heap.objects
            .borrow_mut()
            .sweep_batch(
                heap.owner,
                &heap.interfaces,
                &mut heap.graph.lock(),
                &mut ScanBudget::new(1024),
            )
            .is_none()
    );
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
    assert!(
        heap.objects
            .borrow_mut()
            .sweep_batch(
                heap.owner,
                &heap.interfaces,
                &mut heap.graph.lock(),
                &mut ScanBudget::new(1024),
            )
            .is_none()
    );
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 10);
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
        let empty = batch(heap, 0);
        assert!(!empty.complete);
        assert!(empty.disposal.is_none());
        assert_eq!(heap.dispose_sweep_batch(empty), 0);
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
        assert!(
            heap.objects
                .borrow_mut()
                .sweep_batch(
                    heap.owner,
                    &heap.interfaces,
                    &mut heap.graph.lock(),
                    &mut ScanBudget::new(1024),
                )
                .is_none()
        );
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
