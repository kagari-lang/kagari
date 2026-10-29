use super::*;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution_metadata::{
        MetadataEdge, MetadataRoot,
        application_key::{
            ApplicationArguments, ApplicationKey, InterfaceMethodIdentity, MethodIdentity,
        },
        applications::MethodApplication,
        links::MetadataCache,
        scan_records::ScanRecordId,
    },
    frame::types::construction::ConstructionScope,
    gc::{
        graph::GraphIdentity, interfaces::InterfaceMethodBinding,
        weak_cache::WEAK_CACHE_REMOVE_WORK,
    },
    module::{ExecutableModule, Program},
    value::Value,
    value_semantics::identity_equal,
};
use kagari_bytecode::instruction::StructId;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{callable::Signature, scalar::BuiltinType, ty::Ty};
use std::sync::{Arc, OnceLock};

fn load_source(source: &str) -> (Runtime, Program, ExecutableModule) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("interfaces.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut runtime = Runtime::default();
    let loaded_program = runtime.load_program("interfaces", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    (runtime, loaded_program, loaded)
}

fn fixture() -> (Runtime, Program, ExecutableModule, usize, Value) {
    let (runtime, loaded_program, loaded) = load_source(
        r#"
        trait Read { fn read(self) -> i32; }
        trait Child: Read { fn second(self) -> i32; }
        impl Read for i32 { fn read(self) -> i32 { self } }
        impl Child for i32 { fn second(self) -> i32 { 2 } }
        fn main() -> Child { 7 }
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
                .any(|method| loaded.definition_name(method.method) == Some("second"))
        })
        .unwrap();
    let value = runtime
        .make_interface(&loaded, table, Value::I32(7))
        .unwrap();
    (runtime, loaded_program, loaded, table, value)
}

#[test]
fn shared_description_admission_and_detached_arc_access_obey_mark_closure() {
    for retained in [false, true] {
        let (runtime, _program, _owner, _table, value) = fixture();
        let id = identity(&runtime, &value);
        let description = runtime
            .gc
            .interface_metadata(id)
            .unwrap()
            .description
            .clone();
        runtime.gc.graph.lock().begin_cycle().unwrap();
        if retained {
            runtime
                .publish_metadata_edge(MetadataEdge::InterfaceDescription(&description))
                .unwrap();
        }
        runtime.gc.graph.lock().finish_roots().unwrap();
        runtime.gc.drain_graph_work(runtime.modules()).unwrap();
        runtime.gc.graph.lock().close_mark().unwrap();
        assert_eq!(
            runtime
                .validate_metadata(MetadataEdge::InterfaceDescription(&description))
                .is_ok(),
            retained
        );
        assert!(
            runtime.gc.interface_metadata(id).is_none(),
            "a description is not a receiver root"
        );
        let mut candidate = None;
        let result = runtime
            .gc
            .admit_interface_description(runtime.modules(), |scan_id| {
                candidate = Some(scan_id);
                InterfaceDescription {
                    scan_id,
                    receiver_operations: MetadataCache::new(),
                    receiver_table: description.receiver_table.clone(),
                    parents: description.parents.clone(),
                    concrete_type: description.concrete_type.clone(),
                    concrete_expression: description.concrete_expression.clone(),
                    interface_type: description.interface_type.clone(),
                    interface_expression: description.interface_expression.clone(),
                    environment: description.environment.clone(),
                    implementation: description.implementation.clone(),
                    methods: description
                        .methods
                        .iter()
                        .map(|method| {
                            method.as_ref().map(|method| InterfaceMethodBinding {
                                identity: OnceLock::new(),
                                receiver_operations: MetadataCache::new(),
                                parameters: method.parameters.clone(),
                                entry_parameters: method.entry_parameters.clone(),
                                entry_arguments: method.entry_arguments.clone(),
                                result_adapter: method.result_adapter.clone(),
                                method: method.method,
                                target: method.target,
                                parameter_types: method.parameter_types.clone(),
                                return_type: method.return_type.clone(),
                            })
                        })
                        .collect(),
                }
            });
        assert_eq!(result.is_ok(), retained);
        assert_eq!(
            runtime
                .gc
                .interface_descriptions
                .borrow()
                .get(candidate.unwrap())
                .is_some(),
            retained
        );
        runtime.gc.graph.lock().finish_sweep().unwrap();
    }
}

fn identity(runtime: &Runtime, value: &Value) -> InterfaceSnapshotId {
    let Value::Interface(id) = value else {
        panic!("interface fixture")
    };
    runtime.gc.interface_snapshot_id(*id).unwrap()
}

#[test]
fn lazy_parent_link_publishes_an_existing_target_after_the_snapshot_is_black() {
    let (runtime, _program, loaded, table, value) = fixture();
    let owner = identity(&runtime, &value);
    let other = runtime
        .make_interface(&loaded, table, Value::I32(7))
        .unwrap();
    let view = runtime.gc.interface_metadata(owner).unwrap();
    let source = view.description.interface_type().clone();
    let target = view.description.parents[0].interface.clone();
    drop(view);
    let parent = runtime.upcast_interface(&other, &source, &target).unwrap();
    let parent_id = identity(&runtime, &parent);
    let other_id = identity(&runtime, &other);
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime
        .publish_metadata_edge(MetadataEdge::Interface(owner))
        .unwrap();
    runtime.gc.graph.lock().finish_roots().unwrap();
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.cache_parent_interface(owner, 0, parent_id).unwrap();
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    assert!(
        runtime
            .gc
            .graph_survivor(&runtime.modules, GraphIdentity::Interface(parent_id))
            .unwrap()
    );
    assert!(
        !runtime
            .gc
            .graph_survivor(&runtime.modules, GraphIdentity::Interface(other_id))
            .unwrap()
    );
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

fn description_key(owner: &ExecutableModule, table: usize) -> InterfaceDescriptionKey {
    InterfaceDescriptionKey {
        owner: owner.key(),
        table,
        view: false,
        arguments: vec![],
        constructions: Default::default(),
    }
}

#[test]
fn weak_description_and_wrapper_promotion_reject_condemned_metadata() {
    let (runtime, _program, owner, table, value) = fixture();
    let snapshot = identity(&runtime, &value);
    let key = description_key(&owner, table);
    let retained = runtime.gc.interface_description(&key).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime.gc.graph.lock().finish_roots().unwrap();
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    assert!(
        !runtime
            .gc
            .graph_survivor(
                &runtime.modules,
                GraphIdentity::InterfaceDescription(retained.scan_id)
            )
            .unwrap()
    );
    let before = runtime.gc.stats();
    assert!(
        runtime.gc.interface_description(&key).is_none(),
        "Arc upgrade is not graph admission"
    );
    assert!(runtime.gc.alloc_interface(snapshot).is_err());
    assert!(
        runtime
            .root_metadata(vec![MetadataRoot::Interface(snapshot)])
            .is_err()
    );
    assert_eq!(runtime.gc.stats(), before);
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

#[test]
fn description_release_is_keyed_to_the_original_record_not_a_new_cache_replacement() {
    let (runtime, _program, owner, table, value) = fixture();
    let snapshot = identity(&runtime, &value);
    let original = runtime
        .gc
        .interface_metadata(snapshot)
        .unwrap()
        .description
        .clone();
    let key = description_key(&owner, table);
    let release = runtime
        .gc
        .interfaces
        .borrow()
        .descriptions
        .token(&key)
        .unwrap();
    let evicted = runtime
        .gc
        .interfaces
        .borrow_mut()
        .descriptions
        .remove(&release, &mut ScanBudget::new(WEAK_CACHE_REMOVE_WORK));
    assert_eq!(evicted.0, ScanStatus::Complete);
    drop(evicted);
    let replacement_value = runtime
        .make_interface(&owner, table, Value::I32(8))
        .unwrap();
    let replacement = runtime
        .gc
        .interface_metadata(identity(&runtime, &replacement_value))
        .unwrap()
        .description
        .clone();
    assert_ne!(original.scan_id, replacement.scan_id);
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime
        .publish_metadata_edge(MetadataEdge::InterfaceDescription(&replacement))
        .unwrap();
    runtime.gc.graph.lock().finish_roots().unwrap();
    runtime.gc.drain_graph_work(runtime.modules()).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    let mut cursor = runtime.gc.interface_descriptions.borrow().begin_sweep();
    let mut retired = Vec::new();
    assert_eq!(
        runtime
            .gc
            .interface_descriptions
            .borrow_mut()
            .detach_slice(
                &mut cursor,
                &mut ScanBudget::new(1),
                1,
                &mut runtime.gc.graph.lock(),
                &mut retired,
            )
            .unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(retired.len(), 1);
    assert_eq!(retired[0].id, original.scan_id);
    runtime
        .gc
        .release_interface_description_cache(
            &retired[0],
            &mut ScanBudget::new(WEAK_CACHE_REMOVE_WORK),
        )
        .unwrap();
    assert_eq!(
        runtime.gc.interface_description(&key).unwrap().scan_id,
        replacement.scan_id
    );

    let release = runtime
        .gc
        .interfaces
        .borrow()
        .descriptions
        .token(&key)
        .unwrap();
    let evicted = runtime
        .gc
        .interfaces
        .borrow_mut()
        .descriptions
        .remove(&release, &mut ScanBudget::new(WEAK_CACHE_REMOVE_WORK));
    assert_eq!(evicted.0, ScanStatus::Complete);
    drop(evicted);
    let reused_value = runtime
        .make_interface(&owner, table, Value::I32(9))
        .unwrap();
    let reused = runtime
        .gc
        .interface_metadata(identity(&runtime, &reused_value))
        .unwrap()
        .description
        .clone();
    assert_eq!(reused.scan_id.parts().1, original.scan_id.parts().1);
    assert_ne!(reused.scan_id.parts().2, original.scan_id.parts().2);
    runtime
        .gc
        .release_interface_description_cache(
            &retired[0],
            &mut ScanBudget::new(WEAK_CACHE_REMOVE_WORK),
        )
        .unwrap();
    assert_eq!(
        runtime.gc.interface_description(&key).unwrap().scan_id,
        reused.scan_id
    );
    retired.clear();
    assert_eq!(
        runtime
            .gc
            .interface_descriptions
            .borrow_mut()
            .detach_slice(
                &mut cursor,
                &mut ScanBudget::new(1),
                32,
                &mut runtime.gc.graph.lock(),
                &mut retired,
            )
            .unwrap(),
        ScanStatus::Complete
    );
    assert!(retired.is_empty());
    runtime.gc.graph.lock().finish_sweep().unwrap();

    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime.gc.graph.lock().finish_roots().unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    let mut cursor = runtime.gc.interface_descriptions.borrow().begin_sweep();
    assert_eq!(
        runtime
            .gc
            .interface_descriptions
            .borrow_mut()
            .detach_slice(
                &mut cursor,
                &mut ScanBudget::new(2),
                32,
                &mut runtime.gc.graph.lock(),
                &mut retired,
            )
            .unwrap(),
        ScanStatus::Complete
    );
    assert_eq!(retired.len(), 2);
    let mut insufficient = ScanBudget::new(WEAK_CACHE_REMOVE_WORK - 1);
    for record in &retired {
        assert_eq!(
            runtime
                .gc
                .release_interface_description_cache(record, &mut insufficient)
                .unwrap(),
            ScanStatus::Pending
        );
    }
    assert_eq!(insufficient.spent(), 0);
    assert!(!runtime.gc.interfaces.borrow().descriptions.is_empty());
    let mut release_budget = ScanBudget::new(retired.len() * WEAK_CACHE_REMOVE_WORK);
    for record in &retired {
        assert_eq!(
            runtime
                .gc
                .release_interface_description_cache(record, &mut release_budget)
                .unwrap(),
            ScanStatus::Complete
        );
    }
    assert_eq!(
        release_budget.spent(),
        retired.len() * WEAK_CACHE_REMOVE_WORK
    );
    assert!(runtime.gc.interfaces.borrow().descriptions.is_empty());
    drop(retired);
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

#[test]
fn boxed_scalar_does_not_acquire_object_identity() {
    let (runtime, _program, _, _, value) = fixture();
    assert!(identity_equal(runtime.gc(), &value, &value).is_err());
}

#[test]
fn rejected_parent_publication_keeps_the_cache_empty_until_valid_upcast() {
    let (runtime, _loaded_program, loaded, table, value) = fixture();
    let id = identity(&runtime, &value);
    let root = runtime.root_value(value).unwrap();
    let stale_value = runtime
        .make_interface(&loaded, table, Value::I32(8))
        .unwrap();
    let stale = identity(&runtime, &stale_value);
    runtime.collect_garbage().unwrap();
    let (foreign, _program, _, _, foreign_value) = fixture();
    let foreign_id = identity(&foreign, &foreign_value);
    for invalid in [stale, foreign_id] {
        assert!(runtime.cache_parent_interface(id, 0, invalid).is_err());
        assert!(runtime.cache_parent_interface(invalid, 0, id).is_err());
    }
    assert!(runtime.cache_parent_interface(id, usize::MAX, id).is_err());
    let view = runtime.gc.interface_metadata(id).unwrap();
    assert!(view.parent_views[0].get().is_none());
    let source = view.description.interface_type().clone();
    let target = view.description.parents[0].interface.clone();
    drop(view);
    let parent = runtime.upcast_interface(&value, &source, &target).unwrap();
    let parent_id = identity(&runtime, &parent);
    assert!(runtime.cache_parent_interface(id, 0, parent_id).is_err());
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc.interface_metadata(id).unwrap().parent_views[0].get(),
        Some(&parent_id)
    );
    assert!(runtime.gc.interface_metadata(parent_id).is_some());
    drop(root);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.interface_metadata(parent_id).is_none());
}

#[test]
fn application_publication_checks_owners_and_retains_only_published_edges() {
    let (runtime, loaded_program, loaded, table, value) = fixture();
    let id = identity(&runtime, &value);
    let root = runtime.root_value(value).unwrap();
    let snapshot = runtime.gc.interface_metadata(id).unwrap();
    let key = ApplicationKey::new(
        MethodIdentity::Interface(Arc::new(InterfaceMethodIdentity {
            owner: loaded.key(),
            table,
            slot: 0,
            interface: snapshot.description.interface_expression.clone(),
            arguments: snapshot.description.receiver_table.arguments.clone(),
            environment: None,
            result_adapter: None,
        })),
        &ApplicationArguments::new(&loaded, vec![], ConstructionScope::new(&loaded)).unwrap(),
        None,
        &Default::default(),
    );
    drop(snapshot);
    let application = || MethodApplication {
        signature: Signature {
            params: vec![],
            result: Ty::Tuple(vec![]),
        },
        scoped_signature: None,
        environment: None,
        result_adapter: None,
    };
    let stale = runtime
        .gc
        .alloc_method_application(runtime.modules(), application())
        .unwrap();
    runtime.collect_garbage().unwrap();
    let foreign = Runtime::default();
    let foreign_id = foreign
        .gc
        .alloc_method_application(foreign.modules(), application())
        .unwrap();
    for invalid in [stale, foreign_id] {
        assert!(
            runtime
                .publish_method_application(&loaded, key.clone(), invalid)
                .is_err()
        );
    }
    assert!(
        foreign
            .publish_method_application(&loaded, key.clone(), foreign_id)
            .is_err()
    );
    assert!(
        runtime
            .modules
            .method_application(&runtime.gc, &loaded, &key)
            .is_none()
    );
    let prepared = runtime
        .gc
        .alloc_method_application(runtime.modules(), application())
        .unwrap();
    runtime
        .publish_method_application(&loaded, key.clone(), prepared)
        .unwrap();
    let unused = runtime
        .gc
        .alloc_method_application(runtime.modules(), application())
        .unwrap();
    assert!(
        runtime
            .publish_method_application(&loaded, key.clone(), unused)
            .is_err()
    );
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(prepared).is_some());
    assert!(runtime.gc.method_application(unused).is_none());
    // Program-owned application facts must not retain the receiver snapshot.
    drop(root);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.interface_metadata(id).is_none());
    assert!(runtime.gc.method_application(prepared).is_some());
    let candidate = runtime
        .stage_reload_verified_program(&loaded_program, None, loaded.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(prepared).is_none());
    let fresh = runtime
        .gc
        .alloc_method_application(runtime.modules(), application())
        .unwrap();
    assert!(
        runtime
            .publish_method_application(&loaded, key, fresh)
            .is_err()
    );
}

#[test]
fn receiver_cache_rejects_invalid_edges_before_publishing_either_cell() {
    let (runtime, _program, _, _, value) = fixture();
    let id = identity(&runtime, &value);
    let _root = runtime.root_value(value).unwrap();
    let stale = runtime
        .gc
        .alloc_operation_group(runtime.modules(), vec![])
        .unwrap();
    runtime.collect_garbage().unwrap();
    let foreign = Runtime::default();
    let foreign_id = foreign
        .gc
        .alloc_operation_group(foreign.modules(), vec![])
        .unwrap();
    for invalid in [stale, foreign_id] {
        assert!(
            runtime
                .cache_receiver_operations(id, 0, Some(invalid))
                .is_err()
        );
    }
    let prepared = runtime
        .gc
        .alloc_operation_group(runtime.modules(), vec![])
        .unwrap();
    assert!(
        runtime
            .cache_receiver_operations(id, usize::MAX, Some(prepared))
            .is_err()
    );
    assert!(
        foreign
            .cache_receiver_operations(id, 0, Some(foreign_id))
            .is_err()
    );
    let view = runtime.gc.interface_metadata(id).unwrap();
    assert!(view.description.receiver_operations.get().is_none());
    assert!(
        view.description.methods[0]
            .as_ref()
            .unwrap()
            .receiver_operations
            .get()
            .is_none()
    );
    drop(view);
    // A successful empty preparation must stay empty even if a later caller
    // incorrectly tries to install a group. The shared cell must not change.
    runtime.cache_receiver_operations(id, 0, None).unwrap();
    assert!(
        runtime
            .cache_receiver_operations(id, 0, Some(prepared))
            .is_err()
    );
    let view = runtime.gc.interface_metadata(id).unwrap();
    assert!(view.description.receiver_operations.get().is_none());
    assert_eq!(
        view.description.methods[0]
            .as_ref()
            .unwrap()
            .receiver_operations
            .get(),
        Some(&None)
    );
    drop(view);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.operation_group(prepared).is_none());
}

#[test]
fn parent_cache_and_prepared_method_keep_views_until_last_root() {
    let (runtime, _loaded_program, _loaded, _table, value) = fixture();
    let id = identity(&runtime, &value);
    let view = runtime.gc.interface_metadata(id).unwrap();
    let source = view.description.interface_type().clone();
    let target = view.description.parents[0].interface.clone();
    drop(view);
    let parent = runtime.upcast_interface(&value, &source, &target).unwrap();
    let parent_id = identity(&runtime, &parent);
    for _ in 0..8 {
        let next = runtime.upcast_interface(&value, &source, &target).unwrap();
        assert_eq!(identity(&runtime, &next), parent_id);
    }
    assert_eq!(runtime.gc.stats().interface_snapshots, 2);
    let method = runtime
        .resolve_interface_method_slot(&value, &target, 0, &[])
        .unwrap();
    assert_eq!(method.receiver(), &Value::I32(7));
    let result = runtime.collect_garbage().unwrap();
    assert_eq!(result.reclaimed_objects, 9);
    assert_eq!(result.reclaimed_interface_snapshots, 0);
    assert_eq!(runtime.gc.stats().interface_snapshots, 2);
    assert!(runtime.gc.interface_metadata(parent_id).is_some());
    assert_eq!(method.parameter_types(&runtime).unwrap().len(), 1);
    drop(method);
    let result = runtime.collect_garbage().unwrap();
    assert_eq!(result.reclaimed_objects, 1);
    assert_eq!(result.reclaimed_interface_snapshots, 2);
    assert!(runtime.gc.interface_metadata(id).is_none());
    assert!(runtime.gc.interface_metadata(parent_id).is_none());
}

#[test]
fn shared_descriptions_keep_receiver_views_separate_and_retire_after_last_root() {
    let (runtime, _loaded_program, loaded, table, first) = fixture();
    let second = runtime
        .make_interface(&loaded, table, Value::I32(23))
        .unwrap();
    let first_root = runtime.root_value(first).unwrap();
    let second_root = runtime.root_value(second).unwrap();
    let first_id = identity(&runtime, &first);
    let second_id = identity(&runtime, &second);
    let first_view = runtime.gc.interface_metadata(first_id).unwrap();
    let second_view = runtime.gc.interface_metadata(second_id).unwrap();
    assert!(Arc::ptr_eq(
        &first_view.description,
        &second_view.description
    ));
    let description = Arc::downgrade(&first_view.description);
    let source = first_view.description.interface_type().clone();
    let target = first_view.description.parents[0].interface.clone();
    drop(first_view);
    drop(second_view);
    let first_parent = runtime.upcast_interface(&first, &source, &target).unwrap();
    let second_parent = runtime.upcast_interface(&second, &source, &target).unwrap();
    let first_parent_id = identity(&runtime, &first_parent);
    let second_parent_id = identity(&runtime, &second_parent);
    assert_ne!(first_parent_id, second_parent_id);
    for (parent, receiver) in [(first_parent, 7), (second_parent, 23)] {
        let method = runtime
            .resolve_interface_method_slot(&parent, &target, 0, &[])
            .unwrap();
        assert_eq!(method.receiver(), &Value::I32(receiver));
    }
    let first_parent_view = runtime.gc.interface_metadata(first_parent_id).unwrap();
    let second_parent_view = runtime.gc.interface_metadata(second_parent_id).unwrap();
    assert!(Arc::ptr_eq(
        &first_parent_view.description,
        &second_parent_view.description
    ));
    drop(first_parent_view);
    drop(second_parent_view);
    drop(first_root);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.interface_metadata(first_id).is_none());
    assert!(runtime.gc.interface_metadata(first_parent_id).is_none());
    assert!(description.upgrade().is_some());
    assert_eq!(
        runtime
            .gc
            .interface_metadata(second_parent_id)
            .unwrap()
            .data,
        Value::I32(23)
    );
    drop(second_root);
    runtime.collect_garbage().unwrap();
    assert!(description.upgrade().is_none());
    assert!(runtime.gc.interfaces.borrow().descriptions.is_empty());
}

#[test]
fn interface_descriptions_do_not_merge_reloaded_implementation_generations() {
    let (runtime, loaded_program, loaded, table, old) = fixture();
    let old_root = runtime.root_value(old).unwrap();
    let staged = runtime
        .stage_reload_verified_program(&loaded_program, None, loaded.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();

    let next = runtime.select_program(&loaded_program).unwrap();
    let new = runtime
        .make_interface(&next, table, Value::I32(19))
        .unwrap();
    let new_root = runtime.root_value(new).unwrap();
    let old_id = identity(&runtime, &old);
    let new_id = identity(&runtime, &new);
    let old_view = runtime.gc.interface_metadata(old_id).unwrap();
    let new_view = runtime.gc.interface_metadata(new_id).unwrap();
    assert!(!Arc::ptr_eq(&old_view.description, &new_view.description));
    assert_eq!(old_view.description.implementation.key(), loaded.key());
    assert_eq!(new_view.description.implementation.key(), next.key());
    let old_description = Arc::downgrade(&old_view.description);
    drop(old_view);
    drop(new_view);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.interface_metadata(old_id).is_some());
    drop(old_root);
    runtime.collect_garbage().unwrap();
    assert!(old_description.upgrade().is_none());
    assert!(runtime.gc.interface_metadata(new_id).is_some());
    drop(new_root);
}

#[test]
fn interface_ids_reject_foreign_stale_bounds_and_exhausted_generations() {
    let (runtime, _loaded_program, loaded, table, value) = fixture();
    let id = identity(&runtime, &value);
    let (foreign, _foreign_loaded_program, _foreign_loaded, _foreign_table, foreign_value) =
        fixture();
    let foreign_id = identity(&foreign, &foreign_value);
    assert_eq!(foreign_id.slot, id.slot);
    assert_eq!(foreign_id.generation, id.generation);
    assert!(foreign.gc.interface_metadata(id).is_none());
    assert!(runtime.gc.interface_metadata(foreign_id).is_none());
    assert!(
        runtime
            .gc
            .interface_metadata(InterfaceSnapshotId {
                slot: usize::MAX,
                ..id
            })
            .is_none()
    );
    runtime.collect_garbage().unwrap();
    let next = runtime
        .make_interface(&loaded, table, Value::I32(9))
        .unwrap();
    let next_id = identity(&runtime, &next);
    assert_eq!(next_id.slot, id.slot);
    assert_ne!(next_id.generation, id.generation);
    let before = runtime.gc.stats();
    assert_eq!(
        runtime
            .validate_metadata(MetadataEdge::Interface(id))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime.gc.alloc_interface(id).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc.stats(), before);
    assert!(runtime.gc.interface_metadata(id).is_none());
    runtime.gc.interfaces.borrow_mut().slots[next_id.slot].generation = u64::MAX;
    runtime.collect_garbage().unwrap();
    let fresh = runtime
        .make_interface(&loaded, table, Value::I32(11))
        .unwrap();
    let fresh_id = identity(&runtime, &fresh);
    assert_ne!(fresh_id.slot, next_id.slot);
    assert!(!runtime.is_quarantined());
}

#[test]
fn borrowed_interface_view_prevents_any_partial_reclamation() {
    let (runtime, _loaded_program, loaded, table, value) = fixture();
    let id = identity(&runtime, &value);
    let group = runtime
        .gc
        .alloc_operation_group(runtime.modules(), vec![])
        .unwrap();
    let view = runtime.gc.interface_metadata(id).unwrap();
    let before = runtime.gc.stats();
    assert_eq!(
        runtime
            .make_interface(&loaded, table, Value::I32(8))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc.stats(), before);
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.gc.stats(), before);
    drop(view);
    assert!(runtime.gc.interface_metadata(id).is_some());
    assert!(runtime.gc.operation_group(group).is_some());
    assert!(runtime.gc.validate_value(&value));
}

#[test]
fn invalid_cached_parent_aborts_before_any_storage_is_detached() {
    let (runtime, _loaded_program, _loaded, _table, value) = fixture();
    let id = identity(&runtime, &value);
    let view = runtime.gc.interface_metadata(id).unwrap();
    assert!(
        runtime
            .cache_parent_interface(
                id,
                0,
                InterfaceSnapshotId {
                    generation: id.generation + 1,
                    ..id
                }
            )
            .is_err()
    );
    assert!(view.parent_views[0].get().is_none());
    view.parent_views[0].corrupt_for_test(InterfaceSnapshotId {
        generation: id.generation + 1,
        ..id
    });
    drop(view);
    let _root = runtime.root_value(value).unwrap();
    let before = runtime.gc.stats();
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.gc.stats(), before);
    assert!(runtime.gc.interface_metadata(id).is_some());
    assert!(runtime.gc.validate_value(&value));
}

#[test]
fn interface_metadata_root_and_prepared_method_do_not_own_records_after_teardown() {
    let (runtime, _loaded_program, loaded) = load_source(
        "trait Read { fn read(self) -> i32; }\n         impl<T> Read for [T] { fn read(self) -> i32 { 7 } }\n         fn main() -> Read { [7] }",
    );
    let array = runtime
        .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| table.arguments == [Ty::Builtin(BuiltinType::I32)])
        .expect("applied array implementation");
    let value = runtime
        .make_interface(&loaded, table, Value::Array(array))
        .unwrap();
    let id = identity(&runtime, &value);
    let view = runtime.gc.interface_metadata(id).unwrap();
    let target = view.description.interface_type().clone();
    let environment_id = view.description.environment.as_ref().unwrap().id;
    drop(view);
    let runtime_owner = runtime.resources().lifetime_probe();
    let root = runtime
        .root_metadata(vec![MetadataRoot::Interface(id)])
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.interface_metadata(id).is_some());
    assert!(runtime.gc.environment(environment_id).is_some());
    assert!(
        !runtime.gc.validate_value(&value),
        "metadata roots do not root the wrapper"
    );
    let value = runtime
        .gc
        .alloc_interface(id)
        .map(Value::Interface)
        .unwrap();
    let method = runtime
        .resolve_interface_method_slot(&value, &target, 0, &[])
        .unwrap();
    drop(root);
    drop(runtime);
    assert!(
        runtime_owner.upgrade().is_none(),
        "prepared method must not own the snapshot"
    );
    let foreign = Runtime::default();
    assert!(method.implementation(&foreign).is_err());
    assert!(method.target(&foreign).is_err());
    assert!(method.parameter_types(&foreign).is_err());
    assert!(method.return_type(&foreign).is_err());
}

#[test]
fn metadata_only_root_retains_the_heap_receiver_without_retaining_the_wrapper() {
    let (runtime, _loaded_program, loaded) = load_source(
        r#"
        trait Read { fn read(self) -> i32; }
        struct Boxed { val value: i32 }
        impl Read for Boxed { fn read(self) -> i32 { self.value } }
        fn main() -> Read { Boxed { value: 7 } }
    "#,
    );
    let receiver = Value::Struct(
        runtime
            .alloc_struct(
                loaded.struct_layout(StructId::new(0)).unwrap(),
                vec![Value::I32(7)],
            )
            .unwrap(),
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
    let wrapper = runtime.make_interface(&loaded, table, receiver).unwrap();
    let id = identity(&runtime, &wrapper);
    let root = runtime
        .root_metadata(vec![MetadataRoot::Interface(id)])
        .unwrap();
    let result = runtime.collect_garbage().unwrap();
    assert_eq!(result.reclaimed_objects, 1);
    assert_eq!(result.reclaimed_interface_snapshots, 0);
    assert_eq!(result.live_objects, 1);
    assert!(runtime.gc.validate_value(&receiver));
    assert_eq!(runtime.gc.interface_metadata(id).unwrap().data, receiver);
    drop(root);
    let result = runtime.collect_garbage().unwrap();
    assert_eq!(result.reclaimed_objects, 1);
    assert_eq!(result.reclaimed_interface_snapshots, 1);
    assert_eq!(result.live_objects, 0);
}

#[test]
fn independent_heap_aliases_keep_their_snapshot_after_wrapper_reuse() {
    let (runtime, _loaded_program, _loaded, _table, old) = fixture();
    let id = identity(&runtime, &old);
    let metadata = runtime
        .root_metadata(vec![MetadataRoot::Interface(id)])
        .unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    assert_eq!(runtime.gc.allocated_objects(), 0);

    let temporary = runtime.gc.alloc_string("reuse".into()).unwrap();
    let first = Value::Interface(runtime.gc.alloc_interface(id).unwrap());
    let second = Value::Interface(runtime.gc.alloc_interface(id).unwrap());
    assert_ne!(
        first, second,
        "distinct heap wrappers keep distinct identities"
    );
    assert_eq!(identity(&runtime, &first), id);
    assert_eq!(identity(&runtime, &second), id);
    assert!(runtime.gc.validate_value(&temporary));
    assert_eq!(runtime.gc.allocated_objects(), 3);
    let first_root = runtime.gc.root_value(first).unwrap();
    let second_root = runtime.gc.root_value(second).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);

    drop(metadata);
    drop(first_root);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    assert_eq!(runtime.gc.allocated_objects(), 1);
    assert!(runtime.gc.validate_value(&second));
    assert!(runtime.gc.interface_metadata(id).is_some());
    drop(second_root);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    assert!(runtime.gc.interface_metadata(id).is_none());
    assert_eq!(runtime.gc.stats().current_heap_units, 0);
}

#[test]
fn invalid_interface_inspection_preserves_declaration_and_slot_error_kinds() {
    let (runtime, _loaded_program, _loaded, _table, value) = fixture();
    let id = identity(&runtime, &value);
    let snapshot = runtime.gc.interface_metadata(id).unwrap();
    let target = snapshot.description.interface_type().clone();
    let method = snapshot
        .description
        .methods
        .iter()
        .flatten()
        .next()
        .unwrap()
        .method;
    drop(snapshot);
    let error = runtime
        .resolve_interface_method(&Value::I32(7), &method)
        .err()
        .unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    let error = runtime
        .resolve_interface_method_slot(&value, &target, usize::MAX, &[])
        .err()
        .unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    runtime.collect_garbage().unwrap();
    let (foreign, _foreign_program, _foreign_owner, _foreign_table, foreign_value) = fixture();
    let _foreign_root = foreign.root_value(foreign_value).unwrap();
    let error = runtime
        .resolve_interface_method(&foreign_value, &method)
        .err()
        .unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    let error = runtime
        .resolve_interface_method_slot(&foreign_value, &target, 0, &[])
        .err()
        .unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
}
