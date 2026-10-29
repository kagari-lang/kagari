use crate::{
    Runtime,
    gc::{HeapObjectId, graph::GraphIdentity},
    native::storage::NativeStorage,
    test_support::layouts::{allocation_owner, layout},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn exact_target_borrow_preserves_owner_generation_kind_and_idle_publication() {
    fn assert_send<T: Send>() {}
    assert_send::<Runtime>();
    let runtime = Runtime::default();
    let foreign = Runtime::default();
    let heap = runtime.gc();
    let value = heap.alloc_string("checked".into()).unwrap();
    let other = foreign.gc().alloc_string("foreign".into()).unwrap();
    let id = value.checked_id().unwrap();
    let stale = HeapObjectId::new(u64::from(id.owner), id.index(), id.generation + 1);
    let roots = heap.active_roots();
    let guard = heap.graph.lock();
    for invalid in [
        other,
        Value::GcHandle(other.checked_id().unwrap()),
        Value::Str(stale),
        Value::GcHandle(stale),
        Value::Tuple(id),
    ] {
        assert!(heap.checked_target(invalid).is_none());
    }
    let target = heap.checked_target(value).unwrap();
    assert!(heap.objects.try_borrow_mut().is_err());
    target.publish().unwrap();
    assert_eq!(heap.active_roots(), roots);
    assert!(heap.objects.try_borrow_mut().is_ok());
    drop(guard);
    runtime.collect_garbage().unwrap();
    assert!(heap.checked_target(value).is_none());
}

#[test]
fn generic_targets_do_not_erase_specialized_array_kind_requirements() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let ty = Ty::Builtin(BuiltinType::I32);
    let native = NativeStorage::takeable::<i32>()
        .prepare_payload(heap, &ty, 42, &owner)
        .unwrap();
    let native = heap.alloc_native(native).unwrap();
    let array = heap.alloc_array(&owner, ty, vec![Value::I32(7)]).unwrap();
    for value in [
        Value::GcHandle(native),
        Value::GcHandle(array),
        Value::Array(array),
    ] {
        assert!(heap.validate_value(&value));
        heap.checked_target(value).unwrap().publish().unwrap();
    }
    for value in [
        Value::Array(native),
        Value::Tuple(native),
        Value::Tuple(array),
    ] {
        assert!(!heap.validate_value(&value));
        assert!(heap.checked_target(value).is_none());
        assert!(heap.publish_value(value).is_err());
    }
    assert_eq!(heap.take_native::<i32>(native).unwrap(), 42);
    assert!(heap.checked_target(Value::GcHandle(native)).is_none());
    assert!(heap.publish_value(Value::GcHandle(native)).is_err());
}

#[test]
fn root_slice_rejects_all_publication_if_any_identity_is_invalid() {
    let runtime = Runtime::default();
    let foreign = Runtime::default();
    let heap = runtime.gc();
    let valid = heap.alloc_string("unpublished prefix".into()).unwrap();
    let invalid = foreign.gc().alloc_string("foreign suffix".into()).unwrap();
    heap.graph.lock().begin_cycle().unwrap();
    let values = [valid, invalid];
    assert!(heap.publish_root_values(&values).is_none());
    assert!(heap.root_execution_values(values.to_vec()).is_none());
    assert_eq!(heap.active_roots(), 0);
    heap.graph.lock().finish_roots().unwrap();
    heap.drain_graph_work(runtime.modules()).unwrap();
    heap.graph.lock().close_mark().unwrap();
    assert!(
        !heap
            .graph_survivor(
                runtime.modules(),
                GraphIdentity::Object(valid.checked_id().unwrap()),
            )
            .unwrap()
    );
    heap.graph.lock().finish_sweep().unwrap();
}

#[test]
fn checked_target_still_checks_phase_and_poison_at_publication() {
    for poison in [false, true] {
        let runtime = Runtime::default();
        let heap = runtime.gc();
        let value = heap.alloc_string("not a root".into()).unwrap();
        let checked = heap.checked_target(value).unwrap();
        if poison {
            assert!(
                catch_unwind(AssertUnwindSafe(|| {
                    let _guard = heap.graph.lock();
                    panic!("injected control poison after target checking");
                }))
                .is_err()
            );
        } else {
            let mut graph = heap.graph.lock();
            graph.begin_cycle().unwrap();
            graph.finish_roots().unwrap();
            graph.close_mark().unwrap();
        }
        assert!(checked.publish().is_err());
        assert_eq!(heap.active_roots(), 0);
    }
}

#[test]
fn root_slice_shades_during_mark_and_scalars_need_no_owner_borrow() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let value = heap.alloc_string("late source".into()).unwrap();
    let dead = heap.alloc_string("condemned".into()).unwrap();
    heap.graph.lock().begin_cycle().unwrap();
    heap.graph.lock().finish_roots().unwrap();
    let values = [Value::I32(1), value];
    heap.publish_root_values(&values).unwrap();
    heap.drain_graph_work(runtime.modules()).unwrap();
    heap.graph.lock().close_mark().unwrap();
    for (value, allowed) in [(value, true), (dead, false)] {
        assert_eq!(
            heap.checked_target(value).unwrap().publish().is_ok(),
            allowed
        );
    }
    heap.graph.lock().finish_sweep().unwrap();
    let _objects = heap.objects.borrow_mut();
    let _graph = heap.graph.lock();
    let scalars = [Value::Unit, Value::I32(42)];
    heap.publish_root_values(&scalars).unwrap();
}

#[test]
fn admitted_struct_copies_preserve_active_barriers_and_sticky_faults() {
    for poison in [false, true] {
        let mut runtime = Runtime::default();
        let shape = layout(&mut runtime, "Record", &[]);
        let heap = runtime.gc();
        let reference = heap.alloc_struct(shape, vec![]).unwrap();
        let _root = runtime.root_value(Value::Struct(reference)).unwrap();
        // SAFETY: this runtime's root retains the struct through each publication;
        // no callback, collection or source mutation intervenes in these copies.
        unsafe { heap.publish_admitted_struct(reference) }.unwrap();
        heap.graph.lock().begin_cycle().unwrap();
        heap.graph.lock().finish_roots().unwrap();
        unsafe { heap.publish_admitted_struct(reference) }.unwrap();
        heap.drain_graph_work(runtime.modules()).unwrap();
        heap.graph.lock().close_mark().unwrap();
        assert!(
            heap.graph_survivor(runtime.modules(), GraphIdentity::Struct(reference))
                .unwrap()
        );
        heap.graph.lock().finish_sweep().unwrap();
        if poison {
            assert!(
                catch_unwind(AssertUnwindSafe(|| {
                    let _marker = heap.graph.lock();
                    panic!("interrupted graph transition");
                }))
                .is_err()
            );
        } else {
            heap.graph.lock().fault();
        }
        assert!(unsafe { heap.publish_admitted_struct(reference) }.is_err());
    }
}
