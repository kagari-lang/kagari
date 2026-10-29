//! Mutations run only between test-driven slices, never through production scheduling.
use crate::{
    Runtime,
    closure::{ClosureTarget, ClosureValueSnapshot},
    execution_metadata::{MetadataEdge, applications::MethodApplication},
    frame::types::EnvironmentRecord,
    gc::graph::{
        GraphIdentity,
        marks::{Color, initialized_words},
        objects::ScanEdge,
        roots::InitialRoots,
    },
    native::storage::{
        NativeStorage,
        scan::{ScanBudget, ScanStatus},
    },
    test_support::layouts::allocation_owner,
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_contract::ids::FunctionRef;

use kagari_types::callable::Signature;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::thread;

fn roots(runtime: &Runtime, cursor: &mut InitialRoots) {
    for _ in 0..10000 {
        let mut budget = ScanBudget::new(5);
        let status = runtime
            .gc
            .scan_initial_root_slice(&runtime.modules, &runtime.host, cursor, &mut budget)
            .unwrap();
        assert!(budget.spent() <= 5);
        if status == ScanStatus::Complete {
            return;
        }
        assert!(budget.spent() > 0);
    }
    panic!("root cursor did not progress");
}

fn work(runtime: &Runtime) {
    for _ in 0..10000 {
        let mut budget = ScanBudget::new(5);
        let status = runtime
            .gc
            .scan_graph_slice(&runtime.modules, &mut budget)
            .unwrap();
        assert!(budget.spent() <= 5);
        if status == ScanStatus::Complete {
            return;
        }
        assert!(budget.spent() > 0);
    }
    panic!("graph cursor did not progress");
}

fn survivor(runtime: &Runtime, value: Value) -> bool {
    runtime
        .gc
        .graph_survivor(&runtime.modules, value.heap_identity().unwrap().graph())
        .unwrap()
}

/// Five targets in five independent original-owner mark blocks.
fn sparse_targets(runtime: &Runtime) -> Vec<Value> {
    (0..513)
        .filter_map(|index| {
            let value = runtime.gc.alloc_string(index.to_string()).unwrap();
            (index % 128 == 0).then_some(value)
        })
        .collect()
}

#[test]
fn initial_roots_reserve_bitmap_words_before_reset_and_retry_the_unshaded_edge() {
    let runtime = Runtime::default();
    let targets = sparse_targets(&runtime);
    let leases: Vec<_> = targets
        .iter()
        .map(|value| runtime.root_value(*value).unwrap())
        .collect();
    let mut cursor = runtime
        .gc
        .begin_initial_roots(&runtime.modules, &runtime.host)
        .unwrap();
    let before = initialized_words();
    let mut budget = ScanBudget::new(16);
    assert_eq!(
        runtime
            .gc
            .scan_initial_root_slice(&runtime.modules, &runtime.host, &mut cursor, &mut budget)
            .unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(budget.spent(), 16);
    assert_eq!(
        initialized_words() - before,
        8,
        "only two blocks fit, not five deferred resets"
    );
    assert!(runtime.gc.graph.lock().close_mark().is_err());
    for _ in 0..10 {
        let before = initialized_words();
        let mut budget = ScanBudget::new(16);
        let status = runtime
            .gc
            .scan_initial_root_slice(&runtime.modules, &runtime.host, &mut cursor, &mut budget)
            .unwrap();
        assert!(initialized_words() - before <= budget.spent());
        assert!(budget.spent() <= 16);
        if status == ScanStatus::Complete {
            break;
        }
    }
    work(&runtime);
    runtime.gc.graph.lock().close_mark().unwrap();
    for value in targets {
        assert!(survivor(&runtime, value));
    }
    runtime.gc.graph.lock().finish_sweep().unwrap();
    drop(leases);
}

#[test]
fn object_edge_cursor_does_not_advance_when_its_bitmap_reset_cannot_fit() {
    let runtime = Runtime::default();
    let targets = sparse_targets(&runtime);
    let parent = runtime.gc.alloc_tuple(targets.clone()).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime.gc.publish_value(parent).unwrap();
    let mut cursor = runtime
        .gc
        .begin_object_scan(parent.direct_reference().unwrap().into())
        .unwrap();
    let mut visit = |edge: ScanEdge<'_>, budget: &mut ScanBudget| {
        runtime
            .gc
            .shade_scanned_edge(&runtime.modules, edge, budget)
    };
    let before = initialized_words();
    let mut short = ScanBudget::new(4);
    assert_eq!(
        runtime
            .gc
            .scan_object_slice(&mut cursor, &mut short, &mut visit)
            .unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(short.spent(), 1);
    assert_eq!(initialized_words(), before);
    for _ in 0..6 {
        let before = initialized_words();
        let mut budget = ScanBudget::new(5);
        let status = runtime
            .gc
            .scan_object_slice(&mut cursor, &mut budget, &mut visit)
            .unwrap();
        assert!(initialized_words() - before <= budget.spent());
        assert!(budget.spent() <= 5);
        if status == ScanStatus::Complete {
            break;
        }
    }
    for value in &targets {
        runtime
            .gc
            .with_heap_mark(value.heap_identity().unwrap().graph(), |mark| {
                assert_eq!(
                    mark.color(runtime.gc.graph.lock().epoch),
                    Color::Gray,
                    "no edge skipped before queue drain"
                );
                Ok(())
            })
            .unwrap();
    }
    runtime.gc.graph.lock().finish_roots().unwrap();
    work(&runtime);
    runtime.gc.graph.lock().close_mark().unwrap();
    for value in targets {
        assert!(survivor(&runtime, value));
    }
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

#[test]
fn sweep_application_and_closure_candidates_check_edges_before_identity_publication() {
    for retained in [false, true] {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let environment = runtime
            .alloc_environment(
                EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap(),
            )
            .unwrap();
        let candidate = || MethodApplication {
            signature: Signature {
                params: vec![],
                result: Ty::Builtin(BuiltinType::Unit),
            },
            scoped_signature: None,
            environment: Some(environment.clone()),
            result_adapter: None,
        };
        let old = runtime
            .gc
            .alloc_method_application(runtime.modules(), candidate())
            .unwrap();

        runtime.gc.graph.lock().begin_cycle().unwrap();
        if retained {
            runtime
                .publish_metadata_edge(MetadataEdge::Program(&owner))
                .unwrap();
            runtime
                .publish_metadata_edge(MetadataEdge::Application(old))
                .unwrap();
        }
        runtime.gc.graph.lock().finish_roots().unwrap();
        work(&runtime);
        runtime.gc.graph.lock().close_mark().unwrap();
        assert_eq!(runtime.gc.environment(environment.id).is_some(), retained);
        assert_eq!(
            runtime.validate_environment(environment.id).is_ok(),
            retained
        );
        assert_eq!(runtime.gc.method_application(old).is_some(), retained);
        let before = runtime.gc.graph.lock().credits;
        assert_eq!(
            runtime
                .gc
                .alloc_method_application(runtime.modules(), candidate())
                .is_ok(),
            retained
        );
        assert_eq!(
            runtime.gc.graph.lock().credits,
            before + usize::from(retained)
        );

        for environment in [None, Some(environment)] {
            let before = runtime.gc.graph.lock().credits;
            let result = runtime.gc.alloc_closure(
                runtime.modules(),
                ClosureValueSnapshot {
                    implementation: owner.clone(),
                    target: ClosureTarget::Script(FunctionRef::new(0)),
                    captures: vec![],
                    environment,
                },
            );
            assert_eq!(result.is_ok(), retained);
            assert_eq!(
                runtime.gc.graph.lock().credits,
                before + usize::from(retained)
            );
        }
        // Edge-free metadata is a valid Sweep newborn, not a blanket phase rejection.
        let newborn = runtime
            .alloc_environment(
                EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap(),
            )
            .unwrap();
        assert!(runtime.gc.environment(newborn.id).is_some());
        runtime.gc.graph.lock().finish_sweep().unwrap();
    }
}

#[test]
fn root_replacement_release_reuse_and_late_registration_do_not_restart_enumeration() {
    let runtime = Runtime::default();
    let old = runtime.gc.alloc_string("old".into()).unwrap();
    let replacement = runtime.gc.alloc_string("replacement".into()).unwrap();
    let late = runtime.gc.alloc_string("late".into()).unwrap();
    let dead = runtime.gc.alloc_string("dead".into()).unwrap();
    let initial = runtime.root_value(old).unwrap();
    let mut cursor = runtime
        .gc
        .begin_initial_roots(&runtime.modules, &runtime.host)
        .unwrap();
    // Capture the first slot generation, then replace its field before the next visit.
    runtime
        .gc
        .scan_initial_root_slice(
            &runtime.modules,
            &runtime.host,
            &mut cursor,
            &mut ScanBudget::new(1),
        )
        .unwrap();
    initial.set(&runtime.gc, replacement).unwrap();
    thread::spawn(move || drop(initial)).join().unwrap();
    // Registration drains release notifications and can reuse a slot behind the cursor.
    let reused = runtime.root_value(late).unwrap();
    roots(&runtime, &mut cursor);
    work(&runtime);
    let behind = runtime.root_value(old).unwrap();
    work(&runtime);
    runtime.gc.graph.lock().close_mark().unwrap();
    for value in [old, replacement, late] {
        assert!(survivor(&runtime, value));
    }
    assert!(!survivor(&runtime, dead));
    assert!(
        runtime.root_value(dead).is_none(),
        "closure forbids resurrection"
    );
    assert!(reused.set(&runtime.gc, dead).is_none());
    assert_eq!(reused.value(&runtime.gc), Some(late));
    runtime.gc.graph.lock().finish_sweep().unwrap();
    drop((reused, behind));
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 4);
}

#[test]
fn native_take_transfers_pending_and_active_work_without_aliasing_the_reused_directory() {
    for steps in [0, 6] {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let payload = NativeStorage::takeable::<i32>()
            .prepare_payload(&runtime.gc, &Ty::Builtin(BuiltinType::I32), 7, &owner)
            .unwrap();
        let id = runtime.gc.alloc_native(payload).unwrap();
        let _old_alias = runtime.root_value(Value::GcHandle(id)).unwrap();
        runtime.gc.graph.lock().begin_cycle().unwrap();
        runtime
            .gc
            .publish_heap_identity(GraphIdentity::Object(id))
            .unwrap();
        runtime.gc.graph.lock().finish_roots().unwrap();
        assert_eq!(
            runtime
                .gc
                .scan_graph_slice(&runtime.modules, &mut ScanBudget::new(steps))
                .unwrap(),
            ScanStatus::Pending
        );
        assert_eq!(runtime.gc.take_native::<i32>(id).unwrap(), 7);
        let payload = NativeStorage::takeable::<i32>()
            .prepare_payload(&runtime.gc, &Ty::Builtin(BuiltinType::I32), 9, &owner)
            .unwrap();
        let replacement = runtime.gc.alloc_native(payload).unwrap();
        assert_eq!(id.index(), replacement.index());
        assert_ne!(id.generation(), replacement.generation());
        work(&runtime);
        runtime.gc.graph.lock().close_mark().unwrap();
        for identity in [GraphIdentity::Moved(id), GraphIdentity::Object(replacement)] {
            assert!(
                runtime
                    .gc
                    .graph_survivor(&runtime.modules, identity)
                    .unwrap()
            );
        }
        assert!(runtime.root_value(Value::GcHandle(id)).is_none());
        runtime.gc.graph.lock().finish_sweep().unwrap();
        let collected = runtime.collect_garbage().unwrap();
        assert_eq!(collected.reclaimed_objects, 1);
        assert_eq!(collected.live_objects, 0);
        assert!(
            runtime
                .gc
                .graph_survivor(&runtime.modules, GraphIdentity::Moved(id))
                .is_err(),
            "marks are not authoritative in Idle"
        );
    }
}

#[test]
fn black_cell_and_array_bulk_publications_preserve_inserted_targets() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let before = runtime.gc.alloc_string("before".into()).unwrap();
    let inserted = runtime.gc.alloc_string("inserted".into()).unwrap();
    let moved = runtime.gc.alloc_string("moved".into()).unwrap();
    let cell = runtime.gc.alloc_cell(ValueType::Str, before).unwrap();
    let target = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::String),
            vec![before, before],
        )
        .unwrap();
    let source = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::String),
            vec![moved, inserted],
        )
        .unwrap();
    let _root = runtime
        .gc
        .root_execution_values(vec![Value::Cell(cell), Value::Array(target)])
        .unwrap();
    let mut cursor = runtime
        .gc
        .begin_initial_roots(&runtime.modules, &runtime.host)
        .unwrap();
    roots(&runtime, &mut cursor);
    work(&runtime);
    runtime.gc.cell_set(cell, ValueType::Str, inserted).unwrap();
    runtime.gc.array_copy_from(target, source).unwrap();
    work(&runtime);
    runtime.gc.graph.lock().close_mark().unwrap();
    assert!(survivor(&runtime, inserted));
    assert!(survivor(&runtime, moved));
    assert!(
        !survivor(&runtime, Value::Array(source)),
        "copy publishes edges, not a source root"
    );
    assert_eq!(
        runtime.gc.array_snapshot(target).unwrap(),
        [moved, inserted]
    );
    runtime.gc.graph.lock().finish_sweep().unwrap();
}
