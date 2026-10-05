use super::*;
use crate::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution_metadata::{MetadataEdge, MetadataRoot},
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::map::DefinitionContext;
use std::sync::Arc;

fn empty(runtime: &Runtime) -> EnvironmentRecord {
    EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap()
}

#[test]
fn foreign_stale_out_of_bounds_and_exhausted_ids_cannot_alias() {
    let definitions = DefinitionContext::new().unwrap();
    let record = || EnvironmentRecord::new(&definitions, vec![], vec![]).unwrap();
    let mut store = EnvironmentStore::new(1);
    let mut other = EnvironmentStore::new(2);
    let old = store.insert(record()).unwrap();
    let foreign = other.insert(record()).unwrap();
    assert!(store.get(foreign).is_none());
    assert!(other.get(old).is_none());
    assert!(
        store
            .get(EnvironmentId {
                slot: usize::MAX,
                ..old
            })
            .is_none()
    );
    assert_eq!(store.detach(&HashSet::new()).len(), 1);
    let next = store.insert(record()).unwrap();
    assert_eq!(next.slot, old.slot);
    assert_ne!(next.generation, old.generation);
    assert!(store.get(old).is_none());
    store.slots[next.slot].generation = u64::MAX;
    assert_eq!(store.detach(&HashSet::new()).len(), 1);
    let fresh = store.insert(record()).unwrap();
    assert_ne!(fresh.slot, next.slot);
    assert_eq!(store.count(), 1);
}

#[test]
fn roots_trace_parents_but_retained_handles_do_not_prevent_reclamation() {
    let runtime = Runtime::default();
    let parent = runtime.gc.alloc_environment(empty(&runtime)).unwrap();
    let mut child = empty(&runtime);
    child.include(Some(parent.clone())).unwrap();
    let child = runtime.gc.alloc_environment(child).unwrap();
    let root = runtime
        .root_metadata(vec![MetadataRoot::Environment(child.id)])
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_environments, 0);
    assert!(runtime.gc.environment(parent.id).is_some());
    assert!(runtime.gc.environment(child.id).is_some());
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_environments, 2);
    assert!(runtime.gc.environment(parent.id).is_none());
    assert!(runtime.gc.environment(child.id).is_none());
    let replacement = runtime.gc.alloc_environment(empty(&runtime)).unwrap();
    assert_eq!(child.id.slot, replacement.id.slot);
    assert_ne!(child.id.generation, replacement.id.generation);
    assert_eq!(
        runtime
            .validate_metadata(MetadataEdge::Environment(child.id))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(
        runtime
            .gc
            .extend_environment(&child, OperationBindings::default())
            .is_err()
    );
    assert!(runtime.gc.environment(replacement.id).is_some());
    assert!(!runtime.is_quarantined());
}

#[test]
fn publication_rejects_expired_or_foreign_edges_without_allocating_records() {
    let runtime = Runtime::default();
    let foreign = Runtime::default();
    let parent = runtime.gc.alloc_environment(empty(&runtime)).unwrap();
    let mut pending = empty(&runtime);
    pending.include(Some(parent.clone())).unwrap();
    // Deliberately retain a draft across collection: publication must recheck its IDs.
    let group = runtime.gc.alloc_operation_group(vec![]).unwrap();
    let mut selected = empty(&runtime);
    selected.add_receiver(&runtime.gc, group).unwrap();
    runtime.collect_garbage().unwrap();
    let before = runtime.gc.stats();
    assert!(runtime.gc.alloc_environment(pending).is_err());
    assert!(runtime.gc.alloc_environment(selected).is_err());
    assert_eq!(runtime.gc.stats(), before);
    assert!(foreign.gc.environment(parent.id).is_none());
    let local = runtime.gc.alloc_environment(empty(&runtime)).unwrap();
    // Even with matching definition provenance, table ownership is independent.
    let mut foreign_record = empty(&runtime);
    foreign_record.include(Some(local)).unwrap();
    let before = foreign.gc.stats();
    assert!(foreign.gc.alloc_environment(foreign_record).is_err());
    assert_eq!(foreign.gc.stats(), before);
}

#[test]
fn extensions_publish_a_new_record_and_do_not_change_existing_handles() {
    let runtime = Runtime::default();
    let original = runtime.gc.alloc_environment(empty(&runtime)).unwrap();
    let group = runtime.gc.alloc_operation_group(vec![]).unwrap();
    let mut operations = OperationBindings::default();
    operations.receiver(&runtime.gc, group).unwrap();
    let extended = runtime
        .gc
        .extend_environment(&original, operations)
        .unwrap();
    assert_ne!(original.id, extended.id);
    assert!(!Arc::ptr_eq(&original.types, &extended.types));
    assert!(
        runtime
            .gc
            .environment(original.id)
            .unwrap()
            .operations()
            .is_empty()
    );
    assert!(
        !runtime
            .gc
            .environment(extended.id)
            .unwrap()
            .operations()
            .is_empty()
    );
    let _root = runtime
        .root_metadata(vec![MetadataRoot::Environment(original.id)])
        .unwrap();
    let collected = runtime.collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_environments, 1);
    assert_eq!(collected.reclaimed_operation_groups, 1);
    assert!(runtime.gc.environment(original.id).is_some());
    assert!(runtime.gc.environment(extended.id).is_none());
}

#[test]
fn borrowed_view_rejects_allocation_and_sweeping_before_any_detachment() {
    let runtime = Runtime::default();
    let cell = runtime
        .gc
        .alloc_cell(ValueType::I32, Value::I32(7))
        .unwrap();
    let environment = runtime.gc.alloc_environment(empty(&runtime)).unwrap();
    let view = runtime.gc.environment(environment.id).unwrap();
    let before = runtime.gc.stats();
    assert!(runtime.gc.alloc_environment(empty(&runtime)).is_err());
    assert!(
        runtime
            .gc
            .extend_environment(&environment, OperationBindings::default())
            .is_err()
    );
    assert_eq!(runtime.gc.stats(), before);
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.gc.stats(), before);
    assert!(runtime.gc.validate_value(&Value::Cell(cell)));
    drop(view);
    assert!(runtime.gc.environment(environment.id).is_some());
}

#[test]
fn environment_growth_reaches_safepoints_without_value_allocations() {
    for threshold in [Some(4), None] {
        let mut config = RuntimeConfig::default();
        config.gc.collection_threshold = threshold;
        let runtime = Runtime::new(config);
        for _ in 0..64 {
            runtime.gc.alloc_environment(empty(&runtime)).unwrap();
            runtime.gc_safepoint().unwrap();
            if threshold.is_some() {
                assert!(runtime.gc.stats().environments < 4);
            }
        }
        let stats = runtime.gc.stats();
        assert_eq!(stats.allocated_objects, 0);
        assert_eq!(stats.environments, if threshold.is_some() { 0 } else { 64 });
        assert_eq!(stats.collections, if threshold.is_some() { 16 } else { 0 });
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc.stats().environments, 0);
    }
}

#[test]
fn a_retained_environment_handle_does_not_own_runtime_storage() {
    let runtime = Runtime::default();
    let handle = runtime.gc.alloc_environment(empty(&runtime)).unwrap();
    let owner = runtime.resources().lifetime_probe();
    drop(runtime);
    assert!(owner.upgrade().is_none());
    let foreign = Runtime::default();
    assert!(foreign.gc.environment(handle.id).is_none());
    assert!(
        foreign
            .validate_metadata(MetadataEdge::Environment(handle.id))
            .is_err()
    );
}
