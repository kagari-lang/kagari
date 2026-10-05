use super::*;
use crate::{Runtime, layout_fixtures::allocation_owner};
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
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
    let mut roots = RootTable::default();
    let first = roots.insert(7, vec![Value::I32(1)]);
    let stale_id = first.id;
    drop(first);
    let next = roots.insert(7, vec![Value::I32(2)]);
    assert_eq!(stale_id.slot, next.id.slot);
    assert!(next.id.generation > stale_id.generation);
    let stale = RootSet {
        id: stale_id,
        lease: next.lease.clone(),
    };
    assert!(roots.entry(&stale).is_none());
    let forged_lease = RootSet {
        id: next.id,
        lease: Arc::new(RootLease),
    };
    assert!(roots.entry(&forged_lease).is_none());
    assert_eq!(roots.entry(&next).unwrap().values, [Value::I32(2)]);
}

#[test]
fn exhausted_root_generation_retires_its_slot() {
    let mut roots = RootTable::default();
    let first = roots.insert(7, vec![Value::Unit]);
    let old_slot = first.id.slot;
    roots.slots[old_slot].generation = u64::MAX;
    drop(first);
    let next = roots.insert(7, vec![Value::Unit]);
    assert_ne!(old_slot, next.id.slot);
    assert!(roots.slots[old_slot].entry.is_none());
    assert_eq!(roots.active(), 1);
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
    let program = || BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut runtime = Runtime::default();
    let old = runtime.load_program("metadata-root", program()).unwrap();
    let roots = runtime
        .root_metadata(vec![MetadataRoot::Program(old.clone())])
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "metadata-root", program())
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
    let program = || BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut runtime = Runtime::default();
    let old = runtime.load_program("roots", program()).unwrap();
    let roots = runtime
        .root_metadata(vec![MetadataRoot::Program(old.clone())])
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "roots", program())
        .unwrap();
    let current = runtime.publish_staged_reload(candidate).unwrap();
    let mut other = Runtime::default();
    let foreign = other.load_program("roots", program()).unwrap();
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
    assert!(runtime.validate_loaded_module(&current).is_ok());
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
