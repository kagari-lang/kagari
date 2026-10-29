//! Leaf completion and resumable mutable edges share the normal graph contracts.
use crate::{
    Runtime,
    gc::graph::{GraphIdentity, marks::Color, objects::ScanEdge},
    native::storage::scan::{ScanBudget, ScanStatus},
    test_support::layouts::layout,
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn leaf_shading_reserves_reset_and_preserves_already_queued_work() {
    let mut runtime = Runtime::default();
    let shape = layout(
        &mut runtime,
        "Point",
        &[("x", Ty::Builtin(BuiltinType::I32), true)],
    );
    let heap = runtime.gc();
    let point = heap.alloc_struct(shape, vec![Value::I32(7)]).unwrap();
    let id = GraphIdentity::Struct(point);
    // Exercise both a fresh white target and gray work previously published by
    // a mutator. A leaf shortcut must not invalidate an already queued owner.
    for published in [false, true] {
        heap.graph.lock().begin_cycle().unwrap();
        if published {
            heap.publish_value(Value::Struct(point)).unwrap();
        } else {
            let mut short = ScanBudget::new(3);
            assert_eq!(
                heap.shade_scanned_edge(runtime.modules(), ScanEdge::Identity(id), &mut short)
                    .unwrap(),
                ScanStatus::Pending,
            );
            assert_eq!(short.spent(), 0);
            heap.with_heap_mark(id, |mark| {
                assert_eq!(mark.color(heap.graph.lock().epoch), Color::White);
                Ok(())
            })
            .unwrap();
        }
        let mut budget = ScanBudget::new(4);
        assert_eq!(
            heap.shade_scanned_edge(
                runtime.modules(),
                ScanEdge::Value(Value::Struct(point)),
                &mut budget
            )
            .unwrap(),
            ScanStatus::Complete,
        );
        assert_eq!(budget.spent(), if published { 0 } else { 4 });
        heap.graph.lock().finish_roots().unwrap();
        if published {
            assert!(heap.graph.lock().close_mark().is_err());
            assert_eq!(
                heap.scan_graph_slice(runtime.modules(), &mut ScanBudget::new(1))
                    .unwrap(),
                ScanStatus::Pending
            );
            assert_eq!(
                heap.scan_graph_slice(runtime.modules(), &mut ScanBudget::new(1))
                    .unwrap(),
                ScanStatus::Complete
            );
        }
        heap.graph.lock().close_mark().unwrap();
        assert!(heap.graph_survivor(runtime.modules(), id).unwrap());
        heap.graph.lock().finish_sweep().unwrap();
    }
}

#[test]
fn suspended_struct_scan_retries_unshaded_field_and_observes_mutation() {
    let mut runtime = Runtime::default();
    let string = Ty::Builtin(BuiltinType::String);
    let shape = layout(
        &mut runtime,
        "Pair",
        &[("a", string.clone(), true), ("b", string, true)],
    );
    let heap = runtime.gc();
    let first = heap.alloc_string("old".into()).unwrap();
    let second = heap.alloc_string("second".into()).unwrap();
    let replacement = heap.alloc_string("new".into()).unwrap();
    let pair = heap
        .alloc_struct(shape.clone(), vec![first, second])
        .unwrap();
    heap.graph.lock().begin_cycle().unwrap();
    heap.publish_value(Value::Struct(pair)).unwrap();
    heap.graph.lock().finish_roots().unwrap();
    // Owner selection + begin + first field fit, but the target bitmap reset
    // cannot. Resumption must retry that field after releasing every payload loan.
    let mut short = ScanBudget::new(3);
    assert_eq!(
        heap.scan_graph_slice(runtime.modules(), &mut short)
            .unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(short.spent(), 3);
    heap.struct_set_slot(pair, &shape, 0, replacement).unwrap();
    for step in 0..20 {
        let mut budget = ScanBudget::new(5);
        let status = heap
            .scan_graph_slice(runtime.modules(), &mut budget)
            .unwrap();
        assert!(budget.spent() <= 5);
        if status == ScanStatus::Complete {
            break;
        }
        assert!(budget.spent() > 0 && step < 19);
    }
    heap.graph.lock().close_mark().unwrap();
    for (value, retained) in [(first, false), (second, true), (replacement, true)] {
        assert_eq!(
            heap.graph_survivor(
                runtime.modules(),
                GraphIdentity::Object(value.checked_id().unwrap())
            )
            .unwrap(),
            retained
        );
    }
    assert!(
        heap.graph_survivor(runtime.modules(), GraphIdentity::Struct(pair))
            .unwrap()
    );
    heap.graph.lock().finish_sweep().unwrap();
}
