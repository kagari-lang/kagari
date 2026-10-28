use super::*;
use crate::{
    Runtime, gc::weak_cache::WeakCache, native::storage::scan::NORMAL_WORK,
    test_support::layouts::layout, value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

#[test]
fn cache_batch_reservation_precedes_detach_and_returns_only_unused_allowance() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let target = Arc::new(());
    let mut cache = WeakCache::default();
    let mut owners = Vec::new();
    for index in 0..RETIRE_BATCH {
        let admission = cache.prepare_insert(index, &target).unwrap();
        owners.push(admission.token());
        assert!(admission.commit().is_none());
    }
    drop(target);
    heap.graph.lock().begin_cycle().unwrap();
    heap.graph.lock().finish_roots().unwrap();
    heap.graph.lock().close_mark().unwrap();

    let reservation = RETIRE_BATCH * (RETIRE_WORK + WEAK_CACHE_REMOVE_WORK);
    let mut short = ScanBudget::new(reservation);
    assert_eq!(
        heap.retire_with_cache_slice::<()>(
            &mut short,
            RETIRE_BATCH,
            WEAK_CACHE_REMOVE_WORK,
            |_, _, _| panic!("must not detach without inspection and release allowance"),
        )
        .unwrap(),
        (ScanStatus::Pending, 0),
    );
    assert_eq!(short.spent(), 0);
    assert_eq!(owners.len(), RETIRE_BATCH);

    for (allowance, expected_count, expected_status) in [
        (reservation + 1, 1, ScanStatus::Pending),
        (NORMAL_WORK, RETIRE_BATCH - 1, ScanStatus::Complete),
    ] {
        let mut budget = ScanBudget::new(allowance);
        let result = heap
            .retire_with_cache_slice(
                &mut budget,
                RETIRE_BATCH,
                WEAK_CACHE_REMOVE_WORK,
                |scan, releases, retired| {
                    retired.reserve(RETIRE_BATCH);
                    while !owners.is_empty() {
                        if !scan.take(1) {
                            return Ok(ScanStatus::Pending);
                        }
                        let token = owners.pop().unwrap();
                        let (status, removed) = cache.remove(&token, releases);
                        assert_eq!(status, ScanStatus::Complete);
                        assert!(removed.is_some());
                        retired.push((token, removed));
                    }
                    Ok(ScanStatus::Complete)
                },
            )
            .unwrap();
        assert_eq!(result, (expected_status, expected_count));
        assert_eq!(
            budget.spent(),
            expected_count * (1 + RETIRE_WORK + WEAK_CACHE_REMOVE_WORK)
        );
        assert!(budget.spent() <= allowance);
    }
    for index in 0..RETIRE_BATCH {
        assert!(cache.token(&index).is_none());
    }
    heap.graph.lock().finish_sweep().unwrap();
}

#[test]
fn metadata_tail_reserves_its_extent_and_exhausted_cursor_needs_no_backing() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    for _ in 0..2 {
        heap.alloc_operation_group(runtime.modules(), vec![])
            .unwrap();
    }
    heap.graph.lock().begin_cycle().unwrap();
    heap.graph.lock().finish_roots().unwrap();
    heap.graph.lock().close_mark().unwrap();
    let mut cursor = heap.operation_groups.borrow().begin_sweep();
    let limit = cursor.remaining_slots().min(RETIRE_BATCH);
    assert_eq!(limit, 2);
    let mut budget = ScanBudget::new(limit * (RETIRE_WORK + 1));
    let result = heap
        .retire_slice(&mut budget, limit, |scan, retired| {
            heap.operation_groups.borrow_mut().detach_slice(
                &mut cursor,
                scan,
                limit,
                &mut heap.graph.lock(),
                retired,
            )
        })
        .unwrap();
    assert_eq!(result, (ScanStatus::Complete, 2));
    assert_eq!(budget.spent(), 2 * (RETIRE_WORK + 1));
    assert_eq!(cursor.remaining_slots(), 0);

    let mut empty = ScanBudget::new(0);
    assert_eq!(
        heap.retire_slice(&mut empty, 0, |scan, retired| {
            let status = heap.operation_groups.borrow_mut().detach_slice(
                &mut cursor,
                scan,
                0,
                &mut heap.graph.lock(),
                retired,
            )?;
            assert_eq!(retired.capacity(), 0);
            Ok(status)
        })
        .unwrap(),
        (ScanStatus::Complete, 0)
    );
    assert_eq!(empty.spent(), 0);
    heap.graph.lock().finish_sweep().unwrap();
}

#[test]
fn detached_struct_token_is_owned_across_insufficient_service_allowance() {
    let mut runtime = Runtime::default();
    let semantic = layout(
        &mut runtime,
        "Retired",
        &[("value", Ty::Builtin(BuiltinType::I32), true)],
    );
    let heap = runtime.gc();
    drop(
        heap.prepare_struct_storage(semantic, vec![Value::I32(1)])
            .unwrap(),
    );
    heap.graph.lock().begin_cycle().unwrap();
    heap.graph.lock().finish_roots().unwrap();
    heap.graph.lock().close_mark().unwrap();
    let mut cursor = heap.begin_retirement(runtime.modules()).unwrap();
    let reserved = RETIRE_WORK + WEAK_CACHE_REMOVE_WORK;
    let mut before_detach = ScanBudget::new(reserved - 1);
    assert_eq!(
        heap.storage_slice(&mut cursor, &mut before_detach).unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(before_detach.spent(), 0);
    assert!(cursor.pending_structure.is_none());
    // Model an owned maintenance handoff, rather than borrowing a cache entry.
    let step = heap.objects.borrow_mut().maintenance_slice(
        &mut cursor.storage,
        &mut ScanBudget::new(4),
        Default::default(),
    );
    let token = step
        .removed_structure
        .expect("unused descriptor release token");
    let original = token.clone();
    cursor.pending_structure = Some((token, step.status));
    let mut short = ScanBudget::new(reserved - 1);
    assert_eq!(
        heap.storage_slice(&mut cursor, &mut short).unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(short.spent(), 0);
    assert!(cursor.pending_structure.is_some());
    assert!(heap.struct_layouts.try_borrow_mut().is_ok());
    assert!(heap.objects.try_borrow_mut().is_ok());

    let mut exact = ScanBudget::new(reserved);
    assert_eq!(
        heap.storage_slice(&mut cursor, &mut exact).unwrap(),
        step.status
    );
    assert_eq!(exact.spent(), reserved);
    assert!(cursor.pending_structure.is_none());
    let (status, removed) = heap
        .struct_layouts
        .borrow_mut()
        .release(&original, &mut ScanBudget::new(WEAK_CACHE_REMOVE_WORK));
    assert_eq!(status, ScanStatus::Complete);
    assert!(
        removed.is_none(),
        "the resumed service removed the original cache admission"
    );
    let mut report = GcCollection::default();
    assert!((0..32).any(|_| {
        let mut budget = ScanBudget::new(NORMAL_WORK);
        let status = heap
            .resources
            .collect_operation(|| {
                heap.retirement_slice(
                    runtime.modules(),
                    &mut cursor,
                    &mut budget,
                    &mut report,
                    false,
                )
            })
            .unwrap()
            .unwrap();
        assert!(budget.spent() <= NORMAL_WORK);
        status == ScanStatus::Complete
    }));
}
