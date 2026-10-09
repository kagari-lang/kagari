use super::*;
use crate::{
    Runtime,
    gc::{GcHeap, GcHeapConfig},
    layout_fixtures::allocation_owner,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn join_validates_native_arguments_and_leaves_the_array_unchanged() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let handle = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::String),
            vec![
                heap.alloc_string("é".into()).unwrap(),
                heap.alloc_string("😀".into()).unwrap(),
            ],
        )
        .unwrap();
    let before = heap.array_snapshot(handle).unwrap();
    let Value::Str(joined) = array_join(
        heap,
        &[Value::Array(handle), heap.alloc_string("/".into()).unwrap()],
    )
    .unwrap() else {
        panic!("joined string");
    };
    assert_eq!(&*heap.string(joined).unwrap(), "é/😀");
    assert_eq!(heap.array_snapshot(handle).unwrap(), before);
    assert!(
        array_join(
            heap,
            &[Value::I32(1), heap.alloc_string("".into()).unwrap()]
        )
        .is_err()
    );
    let invalid = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
    assert!(
        array_join(
            heap,
            &[Value::Array(invalid), heap.alloc_string("".into()).unwrap()]
        )
        .is_err()
    );
    assert_eq!(heap.array_snapshot(invalid).unwrap(), vec![Value::I32(1)]);
    assert!(
        array_join(
            &GcHeap::new(
                GcHeapConfig::default(),
                crate::resource::ResourceState::default()
            ),
            &[Value::Array(handle), heap.alloc_string("".into()).unwrap()]
        )
        .is_err()
    );
}
