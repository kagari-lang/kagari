//! Page marks retire with the final disposed fragment, before physical ID reuse.
use super::tests::{batch, begin};
use crate::{
    Runtime,
    gc::{GcHeap, graph::GraphIdentity},
    native::storage::scan::ScanStatus,
    test_support::layouts::layout,
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn finish_payloads(heap: &GcHeap) {
    loop {
        let mut retired = batch(heap, 1024);
        let status = retired.status;
        heap.dispose_sweep_batch(&mut retired).unwrap();
        if status == ScanStatus::Complete {
            break;
        }
    }
    heap.objects.borrow_mut().finish_physical_sweep().unwrap();
}

#[test]
fn physical_marks_retire_before_small_and_large_page_reuse() {
    for fields in [1, 129] {
        let mut runtime = Runtime::default();
        let names: Vec<_> = (0..fields).map(|index| format!("f{index}")).collect();
        let fields: Vec<_> = names
            .iter()
            .map(|name| (name.as_str(), Ty::Builtin(BuiltinType::I64), true))
            .collect();
        let shape = layout(&mut runtime, "Page", &fields);
        let heap = runtime.gc();
        let old = heap
            .alloc_struct(shape.clone(), vec![Value::I64(1); fields.len()])
            .unwrap();
        // Keep the inert index only; old is never dereferenced after retirement.
        let old_page = heap.objects.borrow().struct_location(old).unwrap().page;
        begin(heap);
        finish_payloads(heap);
        assert_eq!(heap.objects.borrow().pages.usage().mark_bytes, 0);
        #[cfg(feature = "execution-diagnostics")]
        let work_before = heap.graph.lock().storage_usage().2;
        let newborn = heap
            .alloc_struct(shape.clone(), vec![Value::I64(2); fields.len()])
            .unwrap();
        assert_eq!(
            heap.objects.borrow().struct_location(newborn).unwrap().page,
            old_page
        );
        assert_eq!(heap.objects.borrow().pages.usage().mark_pages, 1);
        #[cfg(feature = "execution-diagnostics")]
        assert_eq!(heap.graph.lock().storage_usage().2, work_before + 1);
        assert!(
            heap.graph_survivor(runtime.modules(), GraphIdentity::Struct(newborn))
                .unwrap()
        );
        assert_eq!(
            heap.struct_get_slot(newborn, &shape, 0),
            Some(Value::I64(2))
        );
        heap.graph.lock().finish_sweep().unwrap();
        begin(heap);
        finish_payloads(heap);
        assert_eq!(heap.objects.borrow().pages.usage().mark_bytes, 0);
        #[cfg(feature = "execution-diagnostics")]
        assert_eq!(heap.graph.lock().storage_usage().2, work_before);
        heap.graph.lock().finish_sweep().unwrap();
    }
}

#[test]
fn partial_disposal_retains_marks_for_newborn_in_same_page() {
    let mut runtime = Runtime::default();
    let shape = layout(
        &mut runtime,
        "Point",
        &[("x", Ty::Builtin(BuiltinType::I32), true)],
    );
    let heap = runtime.gc();
    for _ in 0..40 {
        heap.alloc_struct(shape.clone(), vec![Value::I32(1)])
            .unwrap();
    }
    assert_eq!(heap.objects.borrow().pages.usage().mark_pages, 1);
    begin(heap);
    let mut first = batch(heap, 1024);
    assert_eq!(first.objects, 32);
    assert_eq!(
        heap.dispose_sweep_batch(&mut first).unwrap().released_bytes,
        0
    );
    assert_eq!(heap.objects.borrow().pages.usage().mark_pages, 1);
    let newborn = heap
        .alloc_struct(shape.clone(), vec![Value::I32(2)])
        .unwrap();
    finish_payloads(heap);
    assert_eq!(heap.objects.borrow().pages.usage().mark_pages, 1);
    assert!(
        heap.graph_survivor(runtime.modules(), GraphIdentity::Struct(newborn))
            .unwrap()
    );
    assert_eq!(
        heap.struct_get_slot(newborn, &shape, 0),
        Some(Value::I32(2))
    );
    heap.graph.lock().finish_sweep().unwrap();
    begin(heap);
    finish_payloads(heap);
    assert_eq!(heap.objects.borrow().pages.usage().mark_bytes, 0);
    heap.graph.lock().finish_sweep().unwrap();
}
