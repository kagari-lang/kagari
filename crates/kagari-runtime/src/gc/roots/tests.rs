use super::*;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::allocations;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    native::{payload::read::ReadReceiver, storage::NativeStorage},
    test_support::layouts::allocation_owner,
    value::EphemeralValueId,
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{scalar::BuiltinType, ty::Ty};
#[cfg(feature = "execution-diagnostics")]
use std::sync::Barrier;
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

thread_local! {
    static DISPOSAL_RUNTIME: RefCell<Option<Rc<Runtime>>> = const { RefCell::new(None) };
}

#[derive(Debug)]
pub(super) struct DisposalProbe {
    drops: Arc<AtomicUsize>,
    reenter: bool,
}

impl Drop for DisposalProbe {
    fn drop(&mut self) {
        let runtime = DISPOSAL_RUNTIME.with(|slot| slot.borrow().as_ref().unwrap().clone());
        assert!(runtime.gc().roots.try_borrow_mut().is_ok());
        self.drops.fetch_add(1, Ordering::Relaxed);
        if self.reenter {
            assert!(runtime.gc().root_value(Value::Unit).is_none());
        } else {
            panic!("final root dependency destruction");
        }
    }
}

#[test]
fn root_release_stops_after_an_indivisible_record_exhausts_the_budget() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let first = heap.root_execution_values(vec![Value::Unit; 8]).unwrap();
    let first_id = first.id;
    let second = heap.root_execution_values(vec![Value::Unit; 16]).unwrap();
    drop(first);
    drop(second);
    let mut cursor = heap.roots.borrow().begin_release_drain();
    let mut budget = ScanBudget::new(1);
    assert_eq!(
        heap.resources
            .collect_operation(|| heap.drain_root_release_slice(&mut cursor, &mut budget))
            .unwrap()
            .unwrap(),
        ScanStatus::Pending,
    );
    assert_eq!(budget.spent(), 18);
    assert_eq!(cursor.remaining, 1);
    assert_eq!(heap.roots.borrow().release_visits, 1);
    assert!(heap.roots.borrow().slots[first_id.slot].entry.is_some());
    let mut budget = ScanBudget::new(2);
    assert_eq!(
        heap.resources
            .collect_operation(|| heap.drain_root_release_slice(&mut cursor, &mut budget))
            .unwrap()
            .unwrap(),
        ScanStatus::Complete,
    );
    assert_eq!(budget.spent(), 10);
    assert_eq!(heap.roots.borrow().release_visits, 2);
}

#[test]
fn root_disposal_releases_table_borrow_and_contains_panic_or_reentry() {
    for insertion in [false, true] {
        for reenter in [false, true] {
            let runtime = Rc::new(Runtime::default());
            DISPOSAL_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime.clone()));
            let heap = runtime.gc();
            let root = heap.root_execution_values(vec![Value::Unit]).unwrap();
            let drops = Arc::new(AtomicUsize::new(0));
            heap.roots.borrow_mut().slots[root.id.slot]
                .entry
                .as_mut()
                .unwrap()
                .disposal_probe = Some(DisposalProbe {
                drops: drops.clone(),
                reenter,
            });
            drop(root);
            if insertion {
                assert!(heap.root_execution_values(vec![Value::Unit]).is_none());
            } else {
                let mut cursor = heap.roots.borrow().begin_release_drain();
                let mut budget = ScanBudget::new(1);
                assert!(
                    heap.resources
                        .collect_operation(|| {
                            heap.drain_root_release_slice(&mut cursor, &mut budget)
                        })
                        .is_err()
                );
                // One notification plus one stored value and record backing.
                assert_eq!(budget.spent(), 3);
                assert_eq!(cursor.remaining, 0);
            }
            assert_eq!(drops.load(Ordering::Relaxed), 1);
            assert!(runtime.is_quarantined());
            assert_eq!(heap.active_roots(), 0);
            let roots = heap.roots.borrow();
            let released = lock_releases(&roots.released);
            assert_eq!(released.credits, usize::from(insertion));
            assert_eq!(released.pending.len(), usize::from(insertion));
            drop(released);
            drop(roots);
            drop(runtime);
            drop(DISPOSAL_RUNTIME.with(|slot| slot.borrow_mut().take()));
            assert_eq!(drops.load(Ordering::Relaxed), 1);
        }
    }
}

impl RootTable {
    fn insert_test(&mut self, values: Vec<Value>) -> Option<RootSet> {
        let (root, retired) = self.insert(values);
        if let Some(retired) = retired {
            assert!(!retired.dispose());
        }
        root
    }

    fn drain_test_limit(&mut self, limit: usize) -> usize {
        let mut processed = 0;
        while processed < limit {
            let (inspected, retired) = self.drain_released_limit(limit - processed);
            processed += inspected;
            if let Some(retired) = retired {
                assert!(!retired.dispose());
            }
            if inspected == 0 {
                break;
            }
        }
        processed
    }

    fn drain_test_slice(
        &mut self,
        cursor: &mut RootReleaseCursor,
        budget: &mut ScanBudget,
    ) -> ScanStatus {
        loop {
            let (status, retired) = self.drain_release_slice(cursor, budget);
            let had_retired = retired.is_some();
            if let Some(retired) = retired {
                budget.charge(
                    retired
                        .values
                        .len()
                        .saturating_add(retired.metadata.len())
                        .saturating_add(1),
                );
                assert!(!retired.dispose());
            }
            if status == ScanStatus::Complete || !had_retired {
                return status;
            }
        }
    }
}

fn inject_release(roots: &RootTable, id: RootId) {
    let mut released = lock_releases(&roots.released);
    released.reserve().unwrap();
    released.pending.push(id);
}

fn drain_captured_releases(roots: &mut RootTable) {
    let mut cursor = roots.begin_release_drain();
    assert_eq!(
        roots.drain_test_slice(&mut cursor, &mut ScanBudget::new(usize::MAX)),
        ScanStatus::Complete,
    );
}

#[test]
fn native_root_admission_requires_a_loan_and_preserves_validation_and_borrow_checks() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let native = NativeStorage::takeable::<i32>()
        .prepare_payload(heap, &Ty::Builtin(BuiltinType::I32), 1, &owner)
        .unwrap();
    let taken = heap.alloc_native(native).unwrap();
    let stale = Value::GcHandle(taken);
    let taken_root = heap.root_value(stale).unwrap();
    assert_eq!(heap.take_native::<i32>(taken).unwrap(), 1);
    runtime.collect_garbage().unwrap();
    let value = heap.alloc_string("retained".into()).unwrap();
    let source = heap.root_value(value).unwrap();
    let foreign = Runtime::default();
    let foreign_value = foreign.gc().alloc_string("foreign".into()).unwrap();
    let object = NativeStorage::data::<i32>()
        .prepare_payload(heap, &Ty::Builtin(BuiltinType::I32), 42_i32, &owner)
        .unwrap();
    let id = heap.alloc_native(object).unwrap();
    let receiver = heap.root_value(Value::GcHandle(id)).unwrap();
    assert!(heap.retain_native_value(value).is_err());
    assert!(heap.retain_native_value(Value::I32(1)).is_err());
    let retained = heap
        .read_native::<i32, _>(&runtime, ReadReceiver::Value(id), None, |payload, _| {
            assert_eq!(*payload, 42);
            let before = heap.active_roots();
            assert!(heap.root_value(value).is_none());
            assert!(heap.root_execution_values(vec![value]).is_none());
            assert!(runtime.collect_garbage().is_err());
            for invalid in [
                stale,
                foreign_value,
                Value::HostRoot(value.direct_reference().unwrap()),
            ] {
                assert!(heap.retain_native_value(invalid).is_err());
            }
            {
                let _objects = heap.objects.borrow_mut();
                assert!(heap.retain_native_value(value).is_err());
            }
            {
                let _roots = heap.roots.borrow();
                assert!(heap.retain_native_value(value).is_err());
                assert!(heap.retain_native_value(Value::I32(1)).is_err());
            }
            assert_eq!(heap.active_roots(), before);
            let scalar = heap.retain_native_value(Value::I32(42))?;
            assert_eq!(heap.active_roots(), before);
            assert_eq!(scalar.value(heap), Some(Value::I32(42)));
            assert!(heap.retain_value(Value::I32(42)).is_none());
            let retained = heap.retain_native_value(value)?;
            assert!(heap.ensure_no_storage_borrow().is_err());
            assert_eq!(retained.value(heap), Some(value));
            Ok(retained)
        })
        .unwrap();
    drop(source);
    drop(receiver);
    drop(taken_root);
    runtime.collect_garbage().unwrap();
    assert_eq!(retained.value(heap), Some(value));
    drop(retained);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    assert_eq!(heap.allocated_objects(), 0);
}

#[test]
fn native_root_admission_does_not_bypass_closed_execution() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let object = NativeStorage::data::<i32>()
        .prepare_payload(heap, &Ty::Builtin(BuiltinType::I32), 0_i32, &owner)
        .unwrap();
    let id = heap.alloc_native(object).unwrap();
    heap.read_native::<i32, _>(&runtime, ReadReceiver::Value(id), None, |_, _| {
        let before = heap.active_roots();
        heap.resources.close_execution();
        assert!(heap.retain_native_value(Value::Unit).is_err());
        assert_eq!(heap.active_roots(), before);
        Ok(())
    })
    .unwrap();
}

#[test]
fn immutable_scalar_retention_preserves_owner_borrows_and_lifetime_without_slots() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RetainedValue>();
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let foreign = Runtime::default();
    let values = [
        Value::Unit,
        Value::Bool(true),
        Value::Char('λ'),
        Value::I32(-1),
        Value::I64(i64::MIN),
        Value::U64(u64::MAX),
        Value::F32(-0.0),
        Value::F64(1.5),
    ];
    let retained: Vec<_> = values
        .iter()
        .map(|value| heap.retain_value(*value).unwrap())
        .collect();
    assert_eq!(heap.active_roots(), 0);
    assert!(heap.roots.borrow().slots.is_empty());
    assert_eq!(lock_releases(&heap.roots.borrow().released).credits, 0);
    for (value, retained) in values.into_iter().zip(&retained) {
        assert!(retained.as_root().is_none());
        assert_eq!(retained.value(heap), Some(value));
        assert!(retained.value(foreign.gc()).is_none());
        {
            let _roots = heap.roots.borrow_mut();
            assert!(retained.value(heap).is_none());
            assert!(heap.retain_value(value).is_none());
        }
        {
            let _roots = heap.roots.borrow();
            assert!(heap.retain_value(value).is_none());
        }
    }
    assert!(
        heap.retain_value(Value::RuntimeEphemeral(EphemeralValueId(0)))
            .is_none()
    );
    runtime.collect_garbage().unwrap();
    let copies = retained.clone();
    thread::spawn(move || drop(retained)).join().unwrap();
    heap.resources.close_execution();
    assert!(heap.retain_value(Value::I32(1)).is_none());
    assert_eq!(copies[3].value(heap), Some(Value::I32(-1)));
    let resources = runtime.resources().lifetime_probe();
    drop(runtime);
    assert!(resources.upgrade().is_none());
    assert!(copies[3].value(foreign.gc()).is_none());
    thread::spawn(move || drop(copies)).join().unwrap();
}

#[test]
fn scalar_retention_during_collection_preserves_reentry_quarantine() {
    let runtime = Runtime::default();
    let retained = runtime.gc().retain_value(Value::I32(7)).unwrap();
    assert!(
        runtime
            .resources()
            .collect_operation(|| {
                assert!(runtime.gc().retain_value(Value::I32(8)).is_none());
            })
            .is_err()
    );
    assert!(runtime.is_quarantined());
    assert_eq!(retained.value(runtime.gc()), Some(Value::I32(7)));
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn collection_callbacks_cannot_replace_roots_even_if_they_ignore_rejection() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let root = heap.root_value(Value::I32(1)).unwrap();
    assert!(
        heap.resources
            .collect_operation(|| {
                assert!(root.set(heap, Value::I32(2)).is_none());
            })
            .is_err()
    );
    assert!(runtime.is_quarantined());
    assert_eq!(root.value(heap), Some(Value::I32(1)));
}

#[test]
fn root_generation_and_lease_both_protect_reused_slots() {
    let mut roots = RootTable::new(7);
    let first = roots.insert_test(vec![Value::I32(1)]).unwrap();
    let stale_id = first.id;
    drop(first);
    let next = roots.insert_test(vec![Value::I32(2)]).unwrap();
    assert_eq!(stale_id.slot, next.id.slot);
    assert!(next.id.generation > stale_id.generation);
    let stale = RootSet {
        id: stale_id,
        lease: next.lease.clone(),
    };
    assert!(roots.entry(&stale).is_none());
    lock_releases(&roots.released).reserve().unwrap();
    let forged_lease = RootSet {
        id: next.id,
        lease: Arc::new(RootLease {
            id: next.id,
            released: roots.released.clone(),
        }),
    };
    assert!(roots.entry(&forged_lease).is_none());
    assert_eq!(roots.entry(&next).unwrap().values, [Value::I32(2)]);
}

#[test]
fn exhausted_root_generation_retires_its_slot() {
    let mut roots = RootTable::new(7);
    roots.slots.push(RootSlot {
        generation: u64::MAX,
        entry: None,
    });
    roots.free.push(0);
    let first = roots.insert_test(vec![Value::Unit]).unwrap();
    let old_slot = first.id.slot;
    assert_eq!(first.id.generation, u64::MAX);
    drop(first);
    let next = roots.insert_test(vec![Value::Unit]).unwrap();
    assert_ne!(old_slot, next.id.slot);
    assert!(roots.slots[old_slot].entry.is_none());
    assert_eq!(roots.active(), 1);
}

#[test]
fn registration_checks_one_release_regardless_of_pending_or_root_high_water() {
    for peak in [64, 4096] {
        let mut roots = RootTable::new(7);
        let mut retained: Vec<_> = (0..peak)
            .map(|_| roots.insert_test(vec![Value::Unit]).unwrap())
            .collect();
        assert_eq!(roots.release_visits, 0);
        drop(retained.split_off(4));
        let root = roots.insert_test(vec![Value::I32(42)]).unwrap();
        assert_eq!(roots.release_visits, 1);
        assert_eq!(lock_releases(&roots.released).pending.len(), peak - 5);
        assert_eq!(roots.slots.len(), peak);
        drop(root);
        let before = roots.release_visits;
        for _ in 0..32 {
            let root = roots.insert_test(vec![Value::I32(42)]).unwrap();
            assert_eq!(roots.entry(&root).unwrap().values, [Value::I32(42)]);
            drop(root);
        }
        assert_eq!(roots.release_visits - before, 32);
        assert_eq!(roots.slots.len(), peak);
        assert_eq!(roots.active(), 4);
        assert_eq!(lock_releases(&roots.released).pending.len(), peak - 4);
    }
}

#[test]
fn stale_foreign_duplicate_and_live_release_notifications_cannot_retire_a_root() {
    let mut roots = RootTable::new(7);
    let first = roots.insert_test(vec![Value::I32(1)]).unwrap();
    let old = first.id;
    drop(first);
    let next = roots.insert_test(vec![Value::I32(2)]).unwrap();
    assert_eq!(old.slot, next.id.slot);
    for id in [
        old,
        old,
        RootId {
            owner: 8,
            ..next.id
        },
        RootId {
            slot: usize::MAX,
            ..next.id
        },
        next.id,
    ] {
        inject_release(&roots, id);
    }
    drain_captured_releases(&mut roots);
    assert_eq!(roots.release_visits, 6);
    assert!(roots.free.is_empty());
    assert_eq!(roots.entry(&next).unwrap().values, [Value::I32(2)]);
    let current = next.id;
    drop(next);
    inject_release(&roots, current);
    drain_captured_releases(&mut roots);
    assert_eq!(roots.free, [current.slot]);
    assert_eq!(roots.slots[current.slot].generation, current.generation + 1);
    assert_eq!(roots.active(), 0);
}

#[test]
fn failed_release_reservation_does_not_publish_or_consume_a_slot() {
    for reuse in [false, true] {
        let mut roots = RootTable::new(7);
        let root = roots.insert_test(vec![Value::I32(42)]).unwrap();
        if reuse {
            drop(roots.insert_test(Vec::new()).unwrap());
            drain_captured_releases(&mut roots);
        }
        let slots = roots.slots.len();
        let free = roots.free.clone();
        let credits = lock_releases(&roots.released).credits;
        // Force checked-add and Vec capacity overflow separately, without requesting
        // an actual enormous allocation or changing the successful admission path.
        for exhausted in [usize::MAX, usize::MAX - 1] {
            lock_releases(&roots.released).credits = exhausted;
            assert!(roots.insert_test(Vec::new()).is_none());
            assert_eq!(lock_releases(&roots.released).credits, exhausted);
            lock_releases(&roots.released).credits = credits;
            assert_eq!(roots.slots.len(), slots);
            assert_eq!(roots.free, free);
            assert_eq!(roots.entry(&root).unwrap().values, [Value::I32(42)]);
        }
        drop(root);
        drain_captured_releases(&mut roots);
        assert_eq!(lock_releases(&roots.released).credits, 0);
        assert!(roots.insert_test(Vec::new()).is_some());
    }
}

#[test]
fn bounded_release_drain_counts_rejected_ids_and_preserves_pending_credits() {
    let mut roots = RootTable::new(7);
    let root = roots.insert_test(Vec::new()).unwrap();
    let id = root.id;
    drop(root);
    inject_release(&roots, RootId { owner: 8, ..id });
    assert_eq!(roots.drain_test_limit(0), 0);
    assert_eq!(lock_releases(&roots.released).credits, 2);
    assert_eq!(roots.drain_test_limit(1), 1);
    assert!(roots.slots[id.slot].entry.is_some());
    assert_eq!(lock_releases(&roots.released).credits, 1);
    assert_eq!(roots.drain_test_limit(1), 1);
    assert!(roots.slots[id.slot].entry.is_none());
    assert_eq!(lock_releases(&roots.released).credits, 0);
    assert_eq!(roots.drain_test_limit(1), 0);
}

#[test]
fn release_cursor_charges_stale_notifications_without_following_new_arrivals() {
    let mut roots = RootTable::new(7);
    let root = roots.insert_test(Vec::new()).unwrap();
    let id = root.id;
    drop(root);
    inject_release(&roots, RootId { owner: 8, ..id });
    let mut cursor = roots.begin_release_drain();
    let mut zero = ScanBudget::new(0);
    assert_eq!(
        roots.drain_test_slice(&mut cursor, &mut zero),
        ScanStatus::Pending
    );
    assert_eq!(roots.release_visits, 0);
    for expected in [ScanStatus::Pending, ScanStatus::Complete] {
        // LIFO arrivals can replace work, but cannot grow the captured allowance.
        inject_release(&roots, RootId { owner: 8, ..id });
        let mut budget = ScanBudget::new(1);
        assert_eq!(roots.drain_test_slice(&mut cursor, &mut budget), expected);
        assert_eq!(budget.spent(), 1);
    }
    assert_eq!(roots.release_visits, 2);
    assert!(roots.slots[id.slot].entry.is_some());
    assert_eq!(lock_releases(&roots.released).credits, 2);
    assert!(roots.snapshots().is_empty());
    assert_eq!(roots.release_visits, 2);
    drain_captured_releases(&mut roots);
    assert_eq!(lock_releases(&roots.released).credits, 0);
}

#[test]
fn release_cursor_finishes_when_registration_consumes_the_pending_work() {
    let mut roots = RootTable::new(7);
    let first = roots.insert_test(Vec::new()).unwrap();
    let mut empty = roots.begin_release_drain();
    drop(first);
    let mut zero = ScanBudget::new(0);
    assert_eq!(
        roots.drain_test_slice(&mut empty, &mut zero),
        ScanStatus::Complete
    );
    assert_eq!(roots.release_visits, 0);
    let mut cursor = roots.begin_release_drain();
    let next = roots.insert_test(Vec::new()).unwrap();
    assert_eq!(roots.release_visits, 1);
    let mut budget = ScanBudget::new(1);
    assert_eq!(
        roots.drain_test_slice(&mut cursor, &mut budget),
        ScanStatus::Complete
    );
    assert_eq!(budget.spent(), 1);
    assert_eq!(roots.release_visits, 1);
    assert!(roots.entry(&next).is_some());
}

#[test]
fn poisoned_release_transport_does_not_panic_during_unwind_cleanup() {
    let mut roots = RootTable::new(7);
    let root = roots.insert_test(Vec::new()).unwrap();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _released = lock_releases(&roots.released);
            panic!("poison the transport without changing its invariant");
        }))
        .is_err()
    );
    assert!(roots.released.is_poisoned());
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _root = root;
            panic!("operation failure releases its root during unwind");
        }))
        .is_err()
    );
    assert_eq!(roots.drain_test_limit(1), 1);
    assert_eq!(lock_releases(&roots.released).credits, 0);
    assert!(roots.insert_test(Vec::new()).is_some());
}

#[cfg(feature = "execution-diagnostics")]
#[test]
fn last_drop_and_bounded_drain_allocate_nothing_at_full_notification_capacity() {
    let mut roots = RootTable::new(7);
    let mut retained: Vec<_> = (0..257)
        .map(|_| roots.insert_test(Vec::new()).unwrap())
        .collect();
    let capacity = lock_releases(&roots.released).pending.capacity();
    while retained.len() < capacity {
        retained.push(roots.insert_test(Vec::new()).unwrap());
    }
    let copies = retained.clone();
    let (_, counts) = allocations::measure(|| drop(copies));
    assert_eq!(counts.requests, 0);
    assert!(lock_releases(&roots.released).pending.is_empty());
    let other = retained.split_off(retained.len() / 2);
    let start = Barrier::new(3);
    thread::scope(|scope| {
        let first = scope.spawn(|| {
            start.wait();
            allocations::measure(|| drop(retained)).1
        });
        let second = scope.spawn(|| {
            start.wait();
            allocations::measure(|| drop(other)).1
        });
        start.wait();
        assert_eq!(first.join().unwrap().requests, 0);
        assert_eq!(second.join().unwrap().requests, 0);
    });
    {
        let released = lock_releases(&roots.released);
        assert_eq!(released.pending.len(), capacity);
        assert_eq!(released.pending.capacity(), capacity);
        assert_eq!(released.credits, capacity);
    }
    let (processed, counts) = allocations::measure(|| roots.drain_test_limit(7));
    assert_eq!(counts.requests, 0);
    assert_eq!(processed, 7);
    assert_eq!(lock_releases(&roots.released).credits, capacity - 7);
    let (processed, counts) = allocations::measure(|| roots.drain_test_limit(capacity));
    assert_eq!(counts.requests, 0);
    assert_eq!(processed, capacity - 7);
    assert_eq!(roots.free.len(), capacity);
    assert_eq!(lock_releases(&roots.released).credits, 0);
    assert_eq!(lock_releases(&roots.released).pending.capacity(), capacity);
    let reused = roots.insert_test(Vec::new()).unwrap();
    let (_, counts) = allocations::measure(|| {
        drop(reused);
        drain_captured_releases(&mut roots);
    });
    assert_eq!(counts.requests, 0);
}

#[cfg(feature = "execution-diagnostics")]
#[test]
fn late_last_drop_uses_disconnected_transport_without_allocating() {
    let mut roots = RootTable::new(7);
    let released = Arc::downgrade(&roots.released);
    let first = roots.insert_test(Vec::new()).unwrap();
    let last = roots.insert_test(Vec::new()).unwrap();
    let start = Barrier::new(2);
    thread::scope(|scope| {
        let dropping = scope.spawn(|| {
            start.wait();
            allocations::measure(|| drop(first)).1
        });
        start.wait();
        let (_, counts) = allocations::measure(|| drop(roots));
        assert_eq!(counts.requests, 0);
        assert_eq!(dropping.join().unwrap().requests, 0);
    });
    {
        let transport = released.upgrade().unwrap();
        let state = lock_releases(&transport);
        assert!(!state.connected);
        assert_eq!(state.credits, 0);
        assert_eq!(state.pending.capacity(), 0);
    }
    let counts = thread::spawn(move || allocations::measure(|| drop(last)).1)
        .join()
        .unwrap();
    assert_eq!(counts.requests, 0);
    assert!(released.upgrade().is_none());
}

#[test]
fn snapshot_upgrade_holds_last_lease_across_cross_thread_drop_while_table_is_borrowed() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let mut root = Some(heap.root_execution_values(vec![Value::I32(42)]).unwrap());
    let mut table = heap.roots.borrow_mut();
    let mut copied = Vec::new();
    table.visit_snapshots(|entry| {
        let root = root.take().unwrap();
        thread::spawn(move || drop(root)).join().unwrap();
        assert_eq!(entry.lease.strong_count(), 1);
        copied.extend_from_slice(&entry.values);
    });
    assert_eq!(copied, [Value::I32(42)]);
    assert_eq!(table.release_visits, 0);
    drain_captured_releases(&mut table);
    assert_eq!(table.release_visits, 1);
    assert_eq!(table.active(), 0);
    drop(table);
    assert!(heap.root_snapshots().unwrap().is_empty());
}

#[test]
fn delayed_notification_does_not_resurrect_value_or_metadata_snapshots() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let root = heap.root_execution_values(vec![Value::I32(42)]).unwrap();
    root.set_metadata(&runtime, vec![MetadataRoot::Program(owner)])
        .unwrap();
    let id = root.id;
    let mut table = heap.roots.borrow_mut();
    thread::spawn(move || drop(root)).join().unwrap();
    // Withhold delivery while preserving the dead entry, as if final Drop had
    // reached zero strong owners but had not yet published its notification.
    assert_eq!(lock_releases(&table.released).pending.pop(), Some(id));
    assert!(table.slots[id.slot].entry.is_some());
    assert!(table.snapshots().is_empty());
    assert!(table.metadata_snapshots().is_empty());
    assert_eq!(table.release_visits, 0);
    lock_releases(&table.released).pending.push(id);
    drain_captured_releases(&mut table);
    assert_eq!(table.release_visits, 1);
    assert!(table.slots[id.slot].entry.is_none());
}

#[test]
fn host_leases_cross_threads_without_transferring_heap_access() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RootedValue>();
    assert_send_sync::<RootSet>();
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    let root = runtime.root_value(Value::Array(array)).unwrap();
    let copy = root.clone();
    let returned = thread::spawn(move || {
        let retained = copy.clone();
        drop(copy);
        retained
    })
    .join()
    .unwrap();
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    assert_eq!(returned.value(runtime.gc()), Some(Value::Array(array)));
    let foreign = Runtime::default();
    assert!(returned.value(foreign.gc()).is_none());
    assert!(returned.set(foreign.gc(), Value::Unit).is_none());
    thread::spawn(move || drop(returned)).join().unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn root_borrow_conflicts_fail_without_mutation_or_panics() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let roots = heap.root_execution_values(vec![Value::I32(1)]).unwrap();
    roots
        .with_value(heap, 0, |value| {
            assert_eq!(*value, Value::I32(1));
            assert!(roots.set(heap, 0, Value::I32(2)).is_none());
            assert!(roots.set_metadata(&runtime, vec![]).is_err());
            assert!(heap.root_value(Value::Unit).is_none());
            assert!(heap.trace_roots().is_none());
        })
        .unwrap();
    assert_eq!(roots.get(heap, 0), Some(Value::I32(1)));
    assert!(roots.get(heap, 1).is_none());
    assert!(roots.set(heap, 1, Value::Unit).is_none());
}

#[test]
fn root_leases_outlive_runtime_without_owning_its_storage() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    let root = runtime.root_value(Value::Array(array)).unwrap();
    root.set_metadata(&runtime, vec![MetadataRoot::Program(owner.clone())])
        .unwrap();
    let resources = runtime.resources().lifetime_probe();
    drop(owner);
    drop(runtime);
    assert!(resources.upgrade().is_none());
    let other = Runtime::default();
    assert!(root.value(other.gc()).is_none());
    assert!(root.set(other.gc(), Value::Unit).is_none());
    thread::spawn(move || drop(root)).join().unwrap();
}

#[test]
fn metadata_roots_retain_programs_until_the_last_external_lease_drops() {
    let program = || {
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(
            DefinitionContext::new().unwrap().snapshot(),
            &Default::default(),
        )
        .unwrap()
    };
    let mut runtime = Runtime::default();
    let old_program = runtime.load_program("metadata-root", program()).unwrap();
    let old = runtime.select_program(&old_program).unwrap();
    let roots = runtime
        .root_metadata(vec![MetadataRoot::Program(old.clone())])
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&old_program, None, program())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let other = Runtime::default();
    assert!(roots.set_metadata(&other, vec![]).is_err());
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    let clone = roots.clone();
    drop(roots);
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    thread::spawn(move || drop(clone)).join().unwrap();
    assert_eq!(
        runtime.collect_garbage().unwrap().reclaimed_modules,
        [old.key()]
    );
}

#[test]
fn rejected_metadata_replacement_keeps_the_original_program_reachable() {
    let program = || {
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(
            DefinitionContext::new().unwrap().snapshot(),
            &Default::default(),
        )
        .unwrap()
    };
    let mut runtime = Runtime::default();
    let old_program = runtime.load_program("roots", program()).unwrap();
    let old = runtime.select_program(&old_program).unwrap();
    let roots = runtime
        .root_metadata(vec![MetadataRoot::Program(old.clone())])
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&old_program, None, program())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();

    let current = runtime.select_program(&old_program).unwrap();
    let mut other = Runtime::default();
    let foreign_program = other.load_program("roots", program()).unwrap();
    let foreign = other.select_program(&foreign_program).unwrap();
    let before = runtime.gc().active_roots();
    assert!(
        roots
            .set_metadata(&runtime, vec![MetadataRoot::Program(foreign.clone())])
            .is_err()
    );
    assert!(
        runtime
            .root_metadata(vec![MetadataRoot::Program(foreign)])
            .is_err()
    );
    assert_eq!(runtime.gc().active_roots(), before);
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    assert!(runtime.modules().loaded(old.key()).is_some());
    roots
        .set_metadata(&runtime, vec![MetadataRoot::Program(current.clone())])
        .unwrap();
    assert_eq!(
        runtime.collect_garbage().unwrap().reclaimed_modules,
        vec![old.key()]
    );
    assert!(
        roots
            .set_metadata(&runtime, vec![MetadataRoot::Program(old)])
            .is_err()
    );
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    assert!(runtime.validate_executable_module(&current).is_ok());
    assert!(!runtime.is_quarantined());
    drop(roots);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn collection_callbacks_cannot_replace_executable_metadata_roots() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let root = runtime
        .root_metadata(vec![MetadataRoot::Program(owner)])
        .unwrap();
    assert!(
        runtime
            .resources()
            .collect_operation(|| {
                assert!(root.set_metadata(&runtime, vec![]).is_err());
            })
            .is_err()
    );
    assert!(runtime.is_quarantined());
    assert_eq!(runtime.gc().metadata_snapshots().unwrap().len(), 1);
}
