use super::*;
use crate::frame::types::EnvironmentRecord;
use crate::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution_metadata::{MetadataEdge, MetadataRoot},
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_types::ty::Ty;

fn application() -> MethodApplication {
    MethodApplication {
        signature: Signature {
            params: vec![],
            result: Ty::Tuple(vec![]),
        },
        scoped_signature: None,
        environment: None,
        result_adapter: None,
    }
}

#[test]
fn foreign_stale_out_of_bounds_and_exhausted_application_ids_cannot_alias() {
    let mut store = ApplicationStore::new(1);
    let mut other = ApplicationStore::new(2);
    let old = store.insert(application()).unwrap();
    let foreign = other.insert(application()).unwrap();
    assert!(store.get(foreign).is_none());
    assert!(other.get(old).is_none());
    assert!(
        store
            .get(ApplicationId {
                slot: usize::MAX,
                ..old
            })
            .is_none()
    );
    assert_eq!(store.detach(&HashSet::new()).len(), 1);
    let next = store.insert(application()).unwrap();
    assert_eq!(next.slot, old.slot);
    assert_ne!(next.generation, old.generation);
    assert!(store.get(old).is_none());
    store.slots[next.slot].generation = u64::MAX;
    assert_eq!(store.detach(&HashSet::new()).len(), 1);
    let fresh = store.insert(application()).unwrap();
    assert_ne!(fresh.slot, next.slot);
    assert_eq!(store.count(), 1);
}

#[test]
fn rooted_application_keeps_its_environment_edges_until_last_lease() {
    let runtime = Runtime::default();
    let group = runtime.gc.alloc_operation_group(vec![]).unwrap();
    let mut environment =
        EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    environment.add_receiver(&runtime.gc, group).unwrap();
    let environment = runtime.gc.alloc_environment(environment).unwrap();
    let environment_id = environment.id;
    let mut prepared = application();
    prepared.environment = Some(environment);
    let id = runtime.gc.alloc_method_application(prepared).unwrap();
    let root = runtime
        .root_metadata(vec![MetadataRoot::Application(id)])
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(id).is_some());
    assert!(runtime.gc.operation_group(group).is_some());
    assert!(runtime.gc.environment(environment_id).is_some());
    drop(root);
    let collected = runtime.collect_garbage().unwrap();
    assert_eq!(collected.reclaimed_method_applications, 1);
    assert_eq!(collected.reclaimed_operation_groups, 1);
    assert!(runtime.gc.method_application(id).is_none());
    assert!(runtime.gc.environment(environment_id).is_none());
    let replacement = runtime.gc.alloc_method_application(application()).unwrap();
    assert_eq!(replacement.slot, id.slot);
    assert_ne!(replacement.generation, id.generation);
    let before = runtime.gc.stats();
    assert_eq!(
        runtime
            .validate_metadata(MetadataEdge::Application(id))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc.stats(), before);
    assert!(!runtime.is_quarantined());
}

#[test]
fn application_only_growth_is_collected_at_safepoints_unless_disabled() {
    for threshold in [Some(4), None] {
        let mut config = RuntimeConfig::default();
        config.gc.collection_threshold = threshold;
        let runtime = Runtime::new(config);
        for _ in 0..64 {
            runtime.gc.alloc_method_application(application()).unwrap();
            runtime.gc_safepoint().unwrap();
            if threshold.is_some() {
                assert!(runtime.gc.stats().method_applications < 4);
            }
        }
        let stats = runtime.gc.stats();
        assert_eq!(stats.allocated_objects, 0);
        assert_eq!(stats.operation_groups, 0);
        assert_eq!(
            stats.method_applications,
            if threshold.is_some() { 0 } else { 64 }
        );
        assert_eq!(stats.collections, if threshold.is_some() { 16 } else { 0 });
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc.stats().method_applications, 0);
    }
}

#[test]
fn borrowed_application_view_prevents_allocation_and_any_partial_sweep() {
    let runtime = Runtime::default();
    let cell = runtime
        .gc
        .alloc_cell(ValueType::I32, Value::I32(7))
        .unwrap();
    let group = runtime.gc.alloc_operation_group(vec![]).unwrap();
    let id = runtime.gc.alloc_method_application(application()).unwrap();
    let view = runtime.gc.method_application(id).unwrap();
    let before = runtime.gc.stats();
    assert_eq!(
        runtime
            .gc
            .alloc_method_application(application())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.gc.stats(), before);
    drop(view);
    assert!(runtime.gc.method_application(id).is_some());
    assert!(runtime.gc.operation_group(group).is_some());
    assert_eq!(
        runtime.gc.cell_get(cell, ValueType::I32).unwrap(),
        Value::I32(7)
    );
}

#[test]
fn invalid_application_root_aborts_before_other_records_are_detached() {
    let runtime = Runtime::default();
    let cell = runtime
        .gc
        .alloc_cell(ValueType::I32, Value::I32(7))
        .unwrap();
    let group = runtime.gc.alloc_operation_group(vec![]).unwrap();
    let id = runtime.gc.alloc_method_application(application()).unwrap();
    let invalid = ApplicationId {
        generation: id.generation + 1,
        ..id
    };
    assert!(
        runtime
            .root_metadata(vec![MetadataRoot::Application(invalid)])
            .is_err()
    );
    let root = runtime.root_metadata(vec![]).unwrap();
    root.corrupt_metadata_for_test(&runtime.gc, vec![MetadataRoot::Application(invalid)]);
    let before = runtime.gc.stats();
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.gc.stats(), before);
    assert!(runtime.gc.method_application(id).is_some());
    assert!(runtime.gc.operation_group(group).is_some());
    assert_eq!(
        runtime.gc.cell_get(cell, ValueType::I32).unwrap(),
        Value::I32(7)
    );
}
