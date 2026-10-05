use super::*;
use crate::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution_metadata::applications::MethodApplication,
    execution_metadata::{MetadataEdge, MetadataRoot, links::MethodSelection},
    frame::types::EnvironmentRecord,
    module::LoadedModule,
    value::Value,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{declaration::requirement::NativeCallableRequirement, ty::Ty};
use std::sync::Arc;

#[test]
fn foreign_stale_and_exhausted_group_ids_cannot_alias_records() {
    let mut first = OperationGroupStore::new(1);
    let mut second = OperationGroupStore::new(2);
    let old = first.insert(vec![]).unwrap();
    let foreign = second.insert(vec![]).unwrap();
    assert!(first.get(foreign).is_none());
    assert!(second.get(old).is_none());
    assert_eq!(first.detach(&HashSet::new()).len(), 1);
    let next = first.insert(vec![]).unwrap();
    assert_eq!(old.slot, next.slot);
    assert_ne!(old.generation, next.generation);
    assert!(first.get(old).is_none());
    first.slots[next.slot].generation = u64::MAX;
    assert_eq!(first.detach(&HashSet::new()).len(), 1);
    let fresh = first.insert(vec![]).unwrap();
    assert_ne!(fresh.slot, next.slot);
    assert_eq!(first.count(), 1);
}

#[test]
fn metadata_growth_reaches_safepoints_without_heap_allocations() {
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(4);
    let runtime = Runtime::new(config);
    for _ in 0..64 {
        runtime.gc.alloc_operation_group(vec![]).unwrap();
        runtime.gc_safepoint().unwrap();
        assert!(runtime.gc.stats().operation_groups < 4);
    }
    assert_eq!(runtime.gc.stats().allocated_objects, 0);
    assert_eq!(runtime.gc.stats().operation_groups, 0);
    assert_eq!(runtime.gc.stats().collections, 16);
}

#[test]
fn detached_environments_reject_recycled_group_slots() {
    let runtime = Runtime::default();
    let old = runtime.gc.alloc_operation_group(vec![]).unwrap();
    let mut environment =
        EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    environment.add_receiver(&runtime.gc, old).unwrap();
    let environment = runtime.gc.alloc_environment(environment).unwrap();
    runtime
        .validate_metadata(MetadataEdge::Environment(environment.id))
        .unwrap();
    assert_eq!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_operation_groups,
        1
    );
    let replacement = runtime.gc.alloc_operation_group(vec![]).unwrap();
    assert_eq!(old.slot, replacement.slot);
    assert_ne!(old.generation, replacement.generation);
    let before = runtime.gc.stats();
    assert_eq!(
        runtime
            .validate_metadata(MetadataEdge::Environment(environment.id))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(!runtime.is_quarantined());
    assert_eq!(runtime.gc.stats(), before);
    assert!(runtime.gc.operation_group(replacement).is_some());
}

#[test]
fn borrowed_group_views_reject_allocation_and_sweeping_before_detachment() {
    let runtime = Runtime::default();
    let id = runtime.gc.alloc_operation_group(vec![]).unwrap();
    let view = runtime.gc.operation_group(id).unwrap();
    let before = runtime.gc.stats();
    assert!(runtime.gc.alloc_operation_group(vec![]).is_err());
    assert_eq!(runtime.gc.stats(), before);
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.gc.stats(), before);
    drop(view);
    assert!(runtime.gc.operation_group(id).is_some());
}

fn load() -> (Runtime, LoadedModule) {
    let source = r#"
        trait Read { fn read(self) -> i32; fn second(self) -> i32; }
        impl Read for i32 { fn read(self) -> i32 { self } fn second(self) -> i32 { 2 } }
        fn main() -> Read { 7 }
    "#;
    load_source(source)
}

fn load_source(source: &str) -> (Runtime, LoadedModule) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("group.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let code = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime.load_program("group", code).unwrap();
    (runtime, loaded)
}

#[test]
fn operation_application_environment_cycles_release_the_actual_metadata_records() {
    for selected in [false, true] {
        let (runtime, loaded) = load();
        let table = loaded
            .bytecode
            .interface_tables
            .iter()
            .position(|table| {
                table
                    .methods
                    .iter()
                    .any(|method| loaded.definition_name(method.method) == Some("read"))
            })
            .unwrap();
        let value = runtime
            .make_interface(&loaded, table, Value::I32(7))
            .unwrap();
        let Value::Interface(id) = value else {
            panic!("interface fixture");
        };
        let snapshot = runtime.gc.interface_snapshot(id).unwrap();
        let group = runtime
            .bind_table_operations(&snapshot.receiver_table)
            .unwrap();
        let requirement = NativeCallableRequirement {
            receiver: snapshot.concrete_type.clone(),
            interface: snapshot.interface_type.clone(),
            member: snapshot.methods[0].as_ref().unwrap().method,
            arguments: vec![],
        };
        let operation_id = runtime
            .gc
            .operation_group(group)
            .unwrap()
            .operation(&requirement)
            .unwrap();
        let operation = runtime.gc.bound_operation(operation_id).unwrap();
        let mut environment =
            EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
        if selected {
            environment
                .add_operation(&runtime.gc, operation_id)
                .unwrap();
        } else {
            environment.add_receiver(&runtime.gc, group).unwrap();
        }
        let environment = runtime.gc.alloc_environment(environment).unwrap();
        runtime
            .cache_method_application(
                MethodSelection::Operation(operation_id),
                runtime
                    .gc
                    .alloc_method_application(MethodApplication {
                        signature: operation.signature.clone(),
                        scoped_signature: None,
                        environment: Some(environment.clone()),
                        result_adapter: None,
                    })
                    .unwrap(),
            )
            .unwrap();
        let environment_id = environment.id;
        let application_id = *operation.application.get().unwrap();
        let root = runtime
            .root_metadata(vec![MetadataRoot::Environment(environment.id)])
            .unwrap();
        drop((environment, operation, snapshot));
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc.stats().operation_groups, 1);
        assert!(runtime.gc.environment(environment_id).is_some());
        assert!(runtime.gc.method_application(application_id).is_some());
        drop(root);
        assert_eq!(
            runtime
                .collect_garbage()
                .unwrap()
                .reclaimed_operation_groups,
            1
        );
        assert_eq!(runtime.gc.stats().operation_groups, 0);
        assert!(runtime.gc.operation_group(group).is_none());
        assert!(runtime.gc.environment(environment_id).is_none());
        assert!(runtime.gc.method_application(application_id).is_none());
    }
}

fn selections() -> (Runtime, LoadedModule, Vec<OperationId>) {
    let (runtime, loaded) = load();
    let table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            table
                .methods
                .iter()
                .any(|method| loaded.definition_name(method.method) == Some("read"))
        })
        .unwrap();
    let Value::Interface(id) = runtime
        .make_interface(&loaded, table, Value::I32(7))
        .unwrap()
    else {
        panic!("interface fixture");
    };
    let snapshot = runtime.gc.interface_snapshot(id).unwrap();
    let group = runtime
        .bind_table_operations(&snapshot.receiver_table)
        .unwrap();
    let operations = snapshot
        .methods
        .iter()
        .flatten()
        .map(|method| {
            runtime
                .gc
                .operation_group(group)
                .unwrap()
                .operation(&NativeCallableRequirement {
                    receiver: snapshot.concrete_type.clone(),
                    interface: snapshot.interface_type.clone(),
                    member: method.method,
                    arguments: vec![],
                })
                .unwrap()
        })
        .collect();
    drop(snapshot);
    (runtime, loaded, operations)
}

#[test]
fn selected_witnesses_keep_the_group_without_exposing_siblings() {
    let (runtime, _loaded, ids) = selections();
    assert_eq!(ids.len(), 2);
    let first = (*runtime.gc.bound_operation(ids[0]).unwrap()).clone();
    let sibling = (*runtime.gc.bound_operation(ids[1]).unwrap()).clone();
    let mut environment =
        EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    environment.add_operation(&runtime.gc, ids[0]).unwrap();
    let environment = runtime.gc.alloc_environment(environment).unwrap();
    let root = runtime
        .root_metadata(vec![MetadataRoot::Environment(environment.id)])
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc.stats().operation_groups, 1);
    assert_eq!(
        environment.operation(&runtime.gc, &first.requirement),
        Some(ids[0])
    );
    assert!(runtime.gc.bound_operation(ids[1]).is_some());
    assert!(
        environment
            .operation(&runtime.gc, &sibling.requirement)
            .is_none()
    );
    assert!(
        environment
            .operation_slot(
                &runtime.gc,
                &sibling.requirement.receiver,
                &sibling.requirement.interface,
                sibling.slot
            )
            .is_none()
    );
    // Whole-receiver selection deliberately exposes its complete verified table.
    let mut receiver =
        EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    receiver.add_receiver(&runtime.gc, ids[0].group).unwrap();
    let receiver = runtime.gc.alloc_environment(receiver).unwrap();
    assert_eq!(
        receiver.operation(&runtime.gc, &sibling.requirement),
        Some(ids[1])
    );
    drop(root);
    assert_eq!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_operation_groups,
        1
    );
    // Detached descriptor views are not retention or access capabilities.
    assert!(
        environment
            .operation(&runtime.gc, &first.requirement)
            .is_none()
    );
    assert!(
        receiver
            .operation(&runtime.gc, &sibling.requirement)
            .is_none()
    );
}

#[test]
fn operation_access_checks_owner_generation_and_member_bounds() {
    let (runtime, _loaded, ids) = selections();
    let id = ids[0];
    let descriptor = (*runtime.gc.bound_operation(id).unwrap()).clone();
    let foreign = Runtime::default();
    let foreign_id = foreign
        .gc
        .alloc_bound_operation(descriptor.clone())
        .unwrap();
    assert!(runtime.gc.bound_operation(foreign_id).is_none());
    assert!(foreign.gc.bound_operation(id).is_none());
    let invalid = OperationId {
        group: id.group,
        member: usize::MAX,
    };
    assert!(runtime.gc.bound_operation(invalid).is_none());
    let before = runtime.gc.stats();
    assert_eq!(
        runtime
            .validate_metadata(MetadataEdge::Operation(invalid))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let mut bindings =
        EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    assert!(bindings.add_operation(&runtime.gc, invalid).is_err());
    assert!(bindings.operations().is_empty());
    assert_eq!(runtime.gc.stats(), before);
    runtime.collect_garbage().unwrap();
    let replacement = runtime
        .gc
        .alloc_bound_operation(descriptor.clone())
        .unwrap();
    assert_eq!(id.group.slot, replacement.group.slot);
    assert_ne!(id.group.generation, replacement.group.generation);
    assert!(runtime.gc.bound_operation(id).is_none());
    assert!(bindings.add_operation(&runtime.gc, id).is_err());
    assert!(bindings.operations().is_empty());
    assert!(runtime.gc.bound_operation(replacement).is_some());
    assert!(!runtime.is_quarantined());
}

#[test]
fn invalid_operation_metadata_aborts_before_detaching_any_groups() {
    let (runtime, _loaded, ids) = selections();
    let invalid = OperationId {
        group: ids[0].group,
        member: usize::MAX,
    };
    assert!(
        runtime
            .root_metadata(vec![MetadataRoot::Operation(invalid)])
            .is_err()
    );
    let root = runtime.root_metadata(vec![]).unwrap();
    root.corrupt_metadata_for_test(&runtime.gc, vec![MetadataRoot::Operation(invalid)]);
    let before = runtime.gc.stats();
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.gc.stats(), before);
    for id in ids {
        assert!(runtime.gc.bound_operation(id).is_some());
    }
}

#[test]
fn associated_type_snapshots_survive_selection_collection_without_owning_environments() {
    let (runtime, loaded) = load_source(
        r#"
        trait Reader { type Item; fn read(self) -> Self::Item; }
        impl Reader for i32 { type Item = i32; fn read(self) -> i32 { self } }
        fn main() -> Reader<Item = i32> { 7 }
    "#,
    );
    let table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            table
                .methods
                .iter()
                .any(|method| loaded.definition_name(method.method) == Some("read"))
        })
        .unwrap();
    let Value::Interface(value) = runtime
        .make_interface(&loaded, table, Value::I32(7))
        .unwrap()
    else {
        panic!("interface fixture")
    };
    let snapshot = runtime.gc.interface_snapshot(value).unwrap();
    let group = runtime
        .bind_table_operations(&snapshot.receiver_table)
        .unwrap();
    let receiver = snapshot.concrete_type.clone();
    let interface = snapshot.interface_type.clone();
    let (member, expected) = interface.associated_types.iter().next().unwrap();
    let member = *member;
    let expected = expected.clone();
    drop(snapshot);
    let mut environment =
        EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    let before = environment.types.clone();
    environment.add_receiver(&runtime.gc, group).unwrap();
    assert!(
        before
            .associated_output(&receiver, &interface, member)
            .is_none()
    );
    let environment = runtime.gc.alloc_environment(environment).unwrap();
    let environment_id = environment.id;
    let mut child = EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    child.include(Some(environment.clone())).unwrap();
    let retained = child.types.clone();
    drop((child, environment));
    assert_eq!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_operation_groups,
        1
    );
    assert!(runtime.gc.environment(environment_id).is_none());
    let (actual, owner) = retained
        .associated_output(&receiver, &interface, member)
        .unwrap();
    assert_eq!(actual, &expected);
    assert_eq!(owner.key(), loaded.key());
    let projection = Ty::Projection {
        receiver: Box::new(receiver),
        interface: Box::new(interface),
        member,
        arguments: vec![],
    };
    assert_eq!(retained.resolve(&projection).unwrap(), expected);
    let mut invalid = EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    let before = invalid.types.clone();
    assert!(invalid.add_receiver(&runtime.gc, group).is_err());
    assert!(invalid.operations().is_empty());
    assert!(Arc::ptr_eq(&before, &invalid.types));
}
