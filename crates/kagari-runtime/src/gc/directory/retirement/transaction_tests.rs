use super::*;
use crate::{
    Runtime,
    gc::directory::{
        pages::batch::Disposal,
        retirement::tests::{batch, begin},
    },
    native::storage::{NativePayload, NativeStorage, scan::NativeScanKind, value::StoredValue},
    resource::HeapCharge,
    test_support::layouts::{allocation_owner, layout},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug)]
#[repr(align(64))]
struct Counted {
    ordinal: usize,
    drops: Arc<[AtomicUsize; 4]>,
}

// SAFETY: Rust-owned counters only, no script references or interior script values.
unsafe impl NativePayload for Counted {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'a StoredValue),
    ) -> ScanStatus {
        if budget.take(1) {
            ScanStatus::Complete
        } else {
            ScanStatus::Pending
        }
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(0, 0)
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        assert_eq!(self.drops[self.ordinal].fetch_add(1, Ordering::SeqCst), 0);
        if self.ordinal == 3 {
            panic!("first detached owner panics during cleanup");
        }
    }
}

fn large_structures(runtime: &mut Runtime, count: usize) -> Vec<PayloadLocation> {
    let names: Vec<_> = (0..129).map(|i| format!("field{i}")).collect();
    let fields: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), Ty::Builtin(BuiltinType::I64), true))
        .collect();
    let shape = layout(runtime, "Large", &fields);
    (0..count)
        .map(|_| {
            let value = Value::Struct(
                runtime
                    .gc()
                    .alloc_struct(shape.clone(), vec![Value::I64(0); 129])
                    .unwrap(),
            );
            runtime
                .gc()
                .objects
                .borrow()
                .direct_location(match value {
                    Value::Struct(reference) => reference,
                    _ => unreachable!(),
                })
                .unwrap()
        })
        .collect()
}

#[test]
fn multi_allocation_full_and_normal_batches_share_the_owner_bound_and_allocate_nothing() {
    for normal in [false, true] {
        let mut runtime = Runtime::default();
        let values = large_structures(&mut runtime, 65);
        let heap = runtime.gc();
        begin(heap);
        let mut counts = Vec::new();
        loop {
            let mut budget = ScanBudget::new(if normal { 1024 } else { usize::MAX });
            #[cfg(feature = "execution-diagnostics")]
            let result = {
                let (result, counts) = crate::diagnostics::allocations::measure(|| {
                    heap.retire_physical_batch(&mut budget)
                });
                assert_eq!(
                    counts.requests, 0,
                    "prepare, detach, finish and disposal use fixed storage"
                );
                result
            };
            #[cfg(not(feature = "execution-diagnostics"))]
            let result = heap.retire_physical_batch(&mut budget);
            let (status, disposed) = result.unwrap().expect("fresh allowance progresses");
            assert!(disposed.objects <= 32);
            if normal {
                assert!(budget.spent() <= 1024);
            }
            if disposed.objects != 0 {
                counts.push(disposed.objects);
            }
            if status == ScanStatus::Complete {
                break;
            }
            assert!(counts.len() < 65);
        }
        assert_eq!(counts.iter().sum::<usize>(), 65);
        if !normal {
            assert_eq!(counts, [32, 32, 1]);
        } else {
            assert!(counts.iter().any(|&count| count < 32));
        }
        assert!(
            values
                .iter()
                .all(|id| heap.objects.borrow().structure(*id).is_none())
        );
        assert_eq!(heap.resources.counters().current_retained_bytes, 0);
        assert_eq!(
            heap.objects
                .borrow_mut()
                .finish_physical_sweep()
                .unwrap()
                .objects,
            65
        );
    }
}

#[test]
fn workspace_and_fragment_reservations_precede_any_detachment() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let value = heap.alloc_string(String::with_capacity(7)).unwrap();
    begin(heap);
    let exact = WORKSPACE_WORK + 3 + OWNER_RETIRE_WORK + FRAGMENT_FINISH_WORK + BATCH_FINISH_WORK;
    let mut short_workspace = ScanBudget::new(WORKSPACE_WORK - 1);
    assert!(
        heap.retire_physical_batch(&mut short_workspace)
            .unwrap()
            .is_none()
    );
    assert_eq!(short_workspace.spent(), 0);
    let mut short_owner = ScanBudget::new(exact - 1);
    let (_, disposal) = heap
        .retire_physical_batch(&mut short_owner)
        .unwrap()
        .unwrap();
    assert_eq!(disposal.objects, 0);
    assert_eq!(short_owner.spent(), WORKSPACE_WORK + 3);
    assert!(heap.validate_value(&value));
    assert!(!heap.objects.borrow().sweep.as_ref().unwrap().outstanding);
    let mut enough = ScanBudget::new(exact);
    let (_, disposal) = heap.retire_physical_batch(&mut enough).unwrap().unwrap();
    assert_eq!(disposal.objects, 1);
    assert_eq!(enough.spent(), exact);
    assert_eq!(heap.objects.borrow().pages.occupied().count(), 0);
    assert_eq!(heap.resources.counters().current_retained_bytes, 0);
}

#[test]
fn second_fragment_prepays_sort_scratch_and_merge_before_admission() {
    for short in [true, false] {
        let mut runtime = Runtime::default();
        let values = large_structures(&mut runtime, 2);
        let heap = runtime.gc();
        begin(heap);
        let exact = WORKSPACE_WORK
            + 2 * (1 + OWNER_RETIRE_WORK + FRAGMENT_FINISH_WORK)
            + BATCH_FINISH_WORK
            + ordering_work(2);
        let mut budget = ScanBudget::new(exact - usize::from(short));
        let (_, disposed) = heap.retire_physical_batch(&mut budget).unwrap().unwrap();
        assert_eq!(disposed.objects, if short { 1 } else { 2 });
        assert_eq!(
            values
                .iter()
                .filter(|id| heap.objects.borrow().structure(**id).is_some())
                .count(),
            usize::from(short)
        );
        assert!(!heap.objects.borrow().sweep.as_ref().unwrap().outstanding);
        if short {
            assert_eq!(
                budget.spent(),
                WORKSPACE_WORK + 2 + OWNER_RETIRE_WORK + FRAGMENT_FINISH_WORK + BATCH_FINISH_WORK
            );
            let (_, remaining) = heap
                .retire_physical_batch(&mut ScanBudget::new(1024))
                .unwrap()
                .unwrap();
            assert_eq!(remaining.objects, 1);
        } else {
            assert_eq!(budget.spent(), exact);
        }
        assert_eq!(heap.resources.counters().current_retained_bytes, 0);
    }
}

#[test]
fn late_accounting_failure_preserves_preview_cursor_and_all_fragment_identities() {
    let mut runtime = Runtime::default();
    let values = large_structures(&mut runtime, 3);
    let heap = runtime.gc();
    begin(heap);
    let ids = values;
    let addresses: Vec<_> = ids
        .iter()
        .map(|location| {
            heap.objects
                .borrow()
                .structure(*location)
                .unwrap()
                .bytes
                .as_ptr()
        })
        .collect();
    let before = heap.resources.counters();
    assert_eq!(before.current_heap_units % 3, 0);
    heap.objects.borrow_mut().sweep.as_mut().unwrap().max_units = before.current_heap_units / 3 * 2;
    let mut budget = ScanBudget::new(usize::MAX);
    assert!(heap.retire_physical_batch(&mut budget).is_err());
    assert_eq!(heap.resources.counters(), before);
    assert!(
        !heap.resources.is_quarantined(),
        "no owner detached on preparation error"
    );
    {
        let objects = heap.objects.borrow();
        for (location, address) in ids.iter().zip(addresses) {
            assert_eq!(
                objects.structure(*location).unwrap().bytes.as_ptr(),
                address
            );
        }
        assert!(!objects.sweep.as_ref().unwrap().outstanding);
    }
    // Repair only the injected bound. A retry must see every original candidate:
    // failed preparation did not publish its end-of-domain preview cursor.
    heap.objects.borrow_mut().sweep.as_mut().unwrap().max_units = before.current_heap_units;
    let (_, disposal) = heap
        .retire_physical_batch(&mut ScanBudget::new(usize::MAX))
        .unwrap()
        .unwrap();
    assert_eq!(disposal.objects, 3);
    assert_eq!(
        heap.objects
            .borrow_mut()
            .finish_physical_sweep()
            .unwrap()
            .objects,
        3
    );
}

#[test]
fn greedy_range_cuts_preserve_page_remainders_and_natural_twelve_and_forty_two_runs() {
    for large_count in [20, 21] {
        let mut runtime = Runtime::default();
        let names: Vec<_> = (0..9).map(|i| format!("field{i}")).collect();
        let fields: Vec<_> = names
            .iter()
            .map(|name| (name.as_str(), Ty::Builtin(BuiltinType::I64), true))
            .collect();
        let forty_two = layout(&mut runtime, "FortyTwo", &fields[..2]);
        let twelve = layout(&mut runtime, "Twelve", &fields);
        for _ in 0..42 {
            runtime
                .gc()
                .alloc_struct(forty_two.clone(), vec![Value::I64(0); 2])
                .unwrap();
        }
        for _ in 0..12 {
            runtime
                .gc()
                .alloc_struct(twelve.clone(), vec![Value::I64(0); 9])
                .unwrap();
        }
        let _large = large_structures(&mut runtime, large_count);
        let heap = runtime.gc();
        begin(heap);
        let mut ranges = Vec::new();
        let mut owners = Vec::new();
        loop {
            let mut retired = batch(heap, usize::MAX);
            owners.push(retired.objects);
            for token in retired.disposal.iter().flatten() {
                if let Disposal::Range(range) = token {
                    ranges.push(range.objects());
                }
            }
            let status = retired.status;
            heap.dispose_sweep_batch(&mut retired).unwrap();
            if status == ScanStatus::Complete {
                break;
            }
        }
        if large_count == 20 {
            assert_eq!(ranges, [12, 32, 10]);
            assert_eq!(owners, [32, 32, 10]);
        } else {
            assert_eq!(ranges, [11, 1, 31, 11]);
            assert_eq!(owners, [32, 32, 11]);
        }
        assert_eq!(
            heap.objects
                .borrow_mut()
                .finish_physical_sweep()
                .unwrap()
                .objects,
            large_count + 54
        );
        assert_eq!(heap.objects.borrow().backing_bytes(), 0);
    }
}

#[test]
fn multi_token_unwind_cleanup_runs_after_borrows_and_contains_each_payload_panic() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let drops = Arc::new(std::array::from_fn(|_| AtomicUsize::new(0)));
    let heap = runtime.gc();
    for ordinal in 0..4 {
        let object = NativeStorage::payload::<Counted>()
            .prepare_payload(
                heap,
                &Ty::Builtin(BuiltinType::Unit),
                Counted {
                    ordinal,
                    drops: drops.clone(),
                },
                &owner,
            )
            .unwrap();
        heap.alloc_native(object).unwrap();
    }
    begin(heap);
    let before = heap.resources.counters();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let retired = batch(heap, usize::MAX);
        assert_eq!((retired.objects, retired.fragments), (4, 4));
        assert!(heap.objects.try_borrow_mut().is_ok());
        assert!(heap.graph.try_lock().is_ok());
        panic!("caller unwinds with initialized token workspace");
    }));
    assert!(result.is_err());
    assert!(heap.resources.is_quarantined());
    assert!(drops.iter().all(|count| count.load(Ordering::SeqCst) == 1));
    assert_eq!(
        heap.resources.counters().current_heap_units,
        before.current_heap_units,
        "abandoned transaction does not publish a successful accounting settlement"
    );
    drop(runtime);
    assert!(drops.iter().all(|count| count.load(Ordering::SeqCst) == 1));
}
