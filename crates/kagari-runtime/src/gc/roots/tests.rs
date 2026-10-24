use super::*;
use crate::test_support::native::program::ProgramFixture;
use crate::{Runtime, test_support::layouts::allocation_owner};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::thread;

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
    let first = roots.insert(vec![Value::I32(1)]);
    let stale_id = first.id;
    drop(first);
    let next = roots.insert(vec![Value::I32(2)]);
    assert_eq!(stale_id.slot, next.id.slot);
    assert!(next.id.generation > stale_id.generation);
    let stale = RootSet {
        id: stale_id,
        lease: next.lease.clone(),
    };
    assert!(roots.entry(&stale).is_none());
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
    let first = roots.insert(vec![Value::Unit]);
    let old_slot = first.id.slot;
    assert_eq!(first.id.generation, u64::MAX);
    drop(first);
    let next = roots.insert(vec![Value::Unit]);
    assert_ne!(old_slot, next.id.slot);
    assert!(roots.slots[old_slot].entry.is_none());
    assert_eq!(roots.active(), 1);
}

#[test]
fn registration_work_follows_releases_not_root_high_water() {
    for peak in [64, 4096] {
        let mut roots = RootTable::new(7);
        let mut retained: Vec<_> = (0..peak).map(|_| roots.insert(vec![Value::Unit])).collect();
        assert_eq!(roots.release_visits, 0);
        drop(retained.split_off(4));
        let root = roots.insert(vec![Value::I32(42)]);
        assert_eq!(roots.release_visits, peak - 4);
        assert_eq!(roots.slots.len(), peak);
        drop(root);
        let before = roots.release_visits;
        for _ in 0..32 {
            let root = roots.insert(vec![Value::I32(42)]);
            assert_eq!(roots.entry(&root).unwrap().values, [Value::I32(42)]);
            drop(root);
        }
        assert_eq!(roots.release_visits - before, 32);
        assert_eq!(roots.slots.len(), peak);
        assert_eq!(roots.active(), 4);
        eprintln!(
            "root peak={peak}: retained registration releases=0, batch={}, reuse releases=32",
            peak - 4
        );
    }
}

#[test]
fn stale_foreign_duplicate_and_live_release_notifications_cannot_retire_a_root() {
    let mut roots = RootTable::new(7);
    let first = roots.insert(vec![Value::I32(1)]);
    let old = first.id;
    drop(first);
    let next = roots.insert(vec![Value::I32(2)]);
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
        roots.released.send(id).unwrap();
    }
    roots.drain_released();
    assert_eq!(roots.release_visits, 6);
    assert!(roots.free.is_empty());
    assert_eq!(roots.entry(&next).unwrap().values, [Value::I32(2)]);
    let current = next.id;
    drop(next);
    roots.released.send(current).unwrap();
    roots.drain_released();
    assert_eq!(roots.free, [current.slot]);
    assert_eq!(roots.slots[current.slot].generation, current.generation + 1);
    assert_eq!(roots.active(), 0);
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
    table.drain_released();
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
    assert_eq!(table.releases.try_recv().unwrap(), id);
    assert!(table.slots[id.slot].entry.is_some());
    assert!(table.snapshots().is_empty());
    assert!(table.metadata_snapshots().is_empty());
    assert_eq!(table.release_visits, 0);
    table.released.send(id).unwrap();
    table.drain_released();
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
