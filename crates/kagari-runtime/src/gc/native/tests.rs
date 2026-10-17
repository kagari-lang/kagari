use crate::{
    Runtime, gc::mutations::PreparedCollectionCommit,
    native::conversion::context::ConversionContext, test_support::layouts::sequence_owner,
    value::Value,
};

#[test]
fn native_revision_survives_relocation_replacement_and_rejects_exhaustion() {
    let mut runtime = Runtime::default();
    let owner = sequence_owner(&mut runtime);
    let mut context = ConversionContext::new(&runtime, &owner).unwrap();
    let garbage = context.encode(vec![0_i32]).unwrap();
    let stale = garbage.value(runtime.gc()).unwrap();
    drop(garbage);
    let root = context.encode(vec![1_i32]).unwrap();
    let value = root.value(runtime.gc()).unwrap();
    let Value::GcHandle(id) = value else {
        panic!("native sequence");
    };
    let heap = runtime.gc();
    heap.sequence_push(id, Value::I32(2)).unwrap();
    let revision = heap.collection_revision(&value).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(heap.collection_revision(&value), Some(revision));
    assert_eq!(heap.collection_revision(&stale), None);

    let copy = heap
        .root_value(Value::GcHandle(heap.clone_buffer(id).unwrap()))
        .unwrap();
    assert_eq!(
        heap.collection_revision(&copy.value(heap).unwrap()),
        Some(0)
    );
    let fresh = context.encode(vec![9_i32]).unwrap();
    assert_eq!(
        heap.collection_revision(&fresh.value(heap).unwrap()),
        Some(0)
    );
    assert_eq!(heap.collection_revision(&stale), None);
    let buffer = context.encode(vec![5_i32]).unwrap();
    heap.commit_prepared_collection(
        PreparedCollectionCommit::ReplaceSequence,
        &[value, buffer.value(heap).unwrap()],
    )
    .unwrap();
    assert_eq!(heap.collection_revision(&value), Some(revision + 1));
    assert!(heap.check_collection_revision(&value, revision).is_err());
    assert_eq!(heap.sequence_snapshot(id).unwrap(), vec![Value::I32(5)]);

    {
        let mut objects = heap.objects.borrow_mut();
        heap.native_mut(&mut objects, id).unwrap().revision = u64::MAX;
    }
    let units = heap.stats().current_heap_units;
    assert!(heap.sequence_push(id, Value::I32(6)).is_err());
    assert_eq!(heap.collection_revision(&value), Some(u64::MAX));
    assert_eq!(heap.sequence_snapshot(id).unwrap(), vec![Value::I32(5)]);
    assert_eq!(heap.stats().current_heap_units, units);
}
