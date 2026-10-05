use super::*;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution_metadata::{
        MetadataEdge, MetadataRoot, applications::MethodApplication, links::MethodSelection,
    },
    frame::types::EnvironmentRecord,
    module::LoadedModule,
    value::Value,
};
use kagari_bytecode::instruction::StructId;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{callable::Signature, ty::Ty};

fn load_source(source: &str) -> (Runtime, LoadedModule) {
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
    let program = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime.load_program("interfaces", program).unwrap();
    (runtime, loaded)
}

fn fixture() -> (Runtime, LoadedModule, usize, Value) {
    let (runtime, loaded) = load_source(
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
    (runtime, loaded, table, value)
}

fn identity(runtime: &Runtime, value: &Value) -> InterfaceSnapshotId {
    let Value::Interface(id) = value else {
        panic!("interface fixture")
    };
    runtime.gc.interface_snapshot_id(*id).unwrap()
}

#[test]
fn rejected_parent_publication_keeps_the_cache_empty_until_valid_upcast() {
    let (runtime, loaded, table, value) = fixture();
    let id = identity(&runtime, &value);
    let root = runtime.root_value(value.clone()).unwrap();
    let stale_value = runtime
        .make_interface(&loaded, table, Value::I32(8))
        .unwrap();
    let stale = identity(&runtime, &stale_value);
    runtime.collect_garbage().unwrap();
    let (foreign, _, _, foreign_value) = fixture();
    let foreign_id = identity(&foreign, &foreign_value);
    for invalid in [stale, foreign_id] {
        assert!(runtime.cache_parent_interface(id, 0, invalid).is_err());
        assert!(runtime.cache_parent_interface(invalid, 0, id).is_err());
    }
    assert!(runtime.cache_parent_interface(id, usize::MAX, id).is_err());
    let view = runtime.gc.interface_metadata(id).unwrap();
    assert!(view.parents[0].prepared.get().is_none());
    let source = view.interface_type.clone();
    let target = view.parents[0].interface.clone();
    drop(view);
    let parent = runtime.upcast_interface(&value, &source, &target).unwrap();
    let parent_id = identity(&runtime, &parent);
    assert!(runtime.cache_parent_interface(id, 0, parent_id).is_err());
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc.interface_metadata(id).unwrap().parents[0]
            .prepared
            .get(),
        Some(&parent_id)
    );
    assert!(runtime.gc.interface_metadata(parent_id).is_some());
    drop(root);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.interface_metadata(parent_id).is_none());
}

#[test]
fn application_cache_checks_both_owners_and_retains_only_the_published_edge() {
    let (runtime, _, _, value) = fixture();
    let id = identity(&runtime, &value);
    let root = runtime.root_value(value).unwrap();
    let application = || MethodApplication {
        signature: Signature {
            params: vec![],
            result: Ty::Tuple(vec![]),
        },
        scoped_signature: None,
        environment: None,
        result_adapter: None,
    };
    let stale = runtime.gc.alloc_method_application(application()).unwrap();
    runtime.collect_garbage().unwrap();
    let foreign = Runtime::default();
    let foreign_id = foreign.gc.alloc_method_application(application()).unwrap();
    let owner = MethodSelection::Interface {
        snapshot: id,
        slot: 0,
    };
    for invalid in [stale, foreign_id] {
        assert!(runtime.cache_method_application(owner, invalid).is_err());
    }
    let prepared = runtime.gc.alloc_method_application(application()).unwrap();
    assert!(foreign.cache_method_application(owner, foreign_id).is_err());
    assert!(
        runtime
            .cache_method_application(
                MethodSelection::Interface {
                    snapshot: id,
                    slot: usize::MAX
                },
                prepared
            )
            .is_err()
    );
    assert!(
        runtime.gc.interface_metadata(id).unwrap().methods[0]
            .as_ref()
            .unwrap()
            .application
            .get()
            .is_none()
    );
    runtime.cache_method_application(owner, prepared).unwrap();
    let unused = runtime.gc.alloc_method_application(application()).unwrap();
    assert!(runtime.cache_method_application(owner, unused).is_err());
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(prepared).is_some());
    assert!(runtime.gc.method_application(unused).is_none());
    drop(root);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(prepared).is_none());
    let fresh = runtime.gc.alloc_method_application(application()).unwrap();
    assert!(runtime.cache_method_application(owner, fresh).is_err());
}

#[test]
fn receiver_cache_rejects_invalid_edges_before_publishing_either_cell() {
    let (runtime, _, _, value) = fixture();
    let id = identity(&runtime, &value);
    let _root = runtime.root_value(value).unwrap();
    let stale = runtime.gc.alloc_operation_group(vec![]).unwrap();
    runtime.collect_garbage().unwrap();
    let foreign = Runtime::default();
    let foreign_id = foreign.gc.alloc_operation_group(vec![]).unwrap();
    for invalid in [stale, foreign_id] {
        assert!(
            runtime
                .cache_receiver_operations(id, 0, Some(invalid))
                .is_err()
        );
    }
    let prepared = runtime.gc.alloc_operation_group(vec![]).unwrap();
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
    assert!(view.receiver_operations.get().is_none());
    assert!(
        view.methods[0]
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
    assert!(view.receiver_operations.get().is_none());
    assert_eq!(
        view.methods[0].as_ref().unwrap().receiver_operations.get(),
        Some(&None)
    );
    drop(view);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.operation_group(prepared).is_none());
}

#[test]
fn parent_cache_and_prepared_method_keep_views_until_last_root() {
    let (runtime, _loaded, _table, value) = fixture();
    let id = identity(&runtime, &value);
    let view = runtime.gc.interface_metadata(id).unwrap();
    let source = view.interface_type.clone();
    let target = view.parents[0].interface.clone();
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
fn interface_ids_reject_foreign_stale_bounds_and_exhausted_generations() {
    let (runtime, loaded, table, value) = fixture();
    let id = identity(&runtime, &value);
    let (foreign, _foreign_loaded, _foreign_table, foreign_value) = fixture();
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
    let (runtime, loaded, table, value) = fixture();
    let id = identity(&runtime, &value);
    let group = runtime.gc.alloc_operation_group(vec![]).unwrap();
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
    let (runtime, _loaded, _table, value) = fixture();
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
    assert!(view.parents[0].prepared.get().is_none());
    view.parents[0]
        .prepared
        .corrupt_for_test(InterfaceSnapshotId {
            generation: id.generation + 1,
            ..id
        });
    drop(view);
    let _root = runtime.root_value(value.clone()).unwrap();
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
    let (runtime, _loaded, _table, value) = fixture();
    let id = identity(&runtime, &value);
    let view = runtime.gc.interface_metadata(id).unwrap();
    let target = view.interface_type.clone();
    drop(view);
    // An empty scope adds no substitutions; its checked ID observes record disposal.
    let environment = runtime
        .gc
        .alloc_environment(
            EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap(),
        )
        .unwrap();
    let environment_id = environment.id;
    let runtime_owner = runtime.resources().lifetime_probe();
    runtime.gc.interfaces.borrow_mut().slots[id.slot]
        .interface
        .as_mut()
        .unwrap()
        .environment = Some(environment);
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
    let (runtime, loaded) = load_source(
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
    let wrapper = runtime
        .make_interface(&loaded, table, receiver.clone())
        .unwrap();
    let id = identity(&runtime, &wrapper);
    let root = runtime
        .root_metadata(vec![MetadataRoot::Interface(id)])
        .unwrap();
    let result = runtime.collect_garbage().unwrap();
    assert_eq!(result.reclaimed_objects, 1);
    assert_eq!(result.reclaimed_interface_snapshots, 0);
    assert!(!runtime.gc.validate_value(&wrapper));
    assert!(runtime.gc.validate_value(&receiver));
    assert_eq!(runtime.gc.interface_metadata(id).unwrap().data, receiver);
    drop(root);
    let result = runtime.collect_garbage().unwrap();
    assert_eq!(result.reclaimed_objects, 1);
    assert_eq!(result.reclaimed_interface_snapshots, 1);
    assert!(!runtime.gc.validate_value(&receiver));
}

#[test]
fn invalid_interface_inspection_preserves_declaration_and_slot_error_kinds() {
    let (runtime, _loaded, _table, value) = fixture();
    let id = identity(&runtime, &value);
    let snapshot = runtime.gc.interface_metadata(id).unwrap();
    let target = snapshot.interface_type.clone();
    let method = snapshot.methods.iter().flatten().next().unwrap().method;
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
    let error = runtime
        .resolve_interface_method(&value, &method)
        .err()
        .unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    let error = runtime
        .resolve_interface_method_slot(&value, &target, 0, &[])
        .err()
        .unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
}
