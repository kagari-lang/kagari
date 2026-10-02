use super::*;
use crate::{Runtime, layout_fixtures::allocation_owner};
use kagari_abi::{scalar::BuiltinType, types::AbiType};

use crate::gc::{GcHeap, GcHeapConfig};

#[test]
fn join_validates_native_arguments_and_leaves_the_array_unchanged() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let handle = runtime
        .alloc_array(
            &owner,
            AbiType::Builtin(BuiltinType::String),
            vec![Value::Str("é".into()), Value::Str("😀".into())],
        )
        .unwrap();
    let before = heap.array_snapshot(handle).unwrap();
    assert_eq!(
        array_join(heap, &[Value::Array(handle), Value::Str("/".into())]).unwrap(),
        Value::Str("é/😀".into())
    );
    assert_eq!(heap.array_snapshot(handle).unwrap(), before);
    assert!(array_join(heap, &[Value::I32(1), Value::Str("".into())]).is_err());
    let invalid = runtime
        .alloc_array(
            &owner,
            AbiType::Builtin(BuiltinType::I32),
            vec![Value::I32(1)],
        )
        .unwrap();
    assert!(array_join(heap, &[Value::Array(invalid), Value::Str("".into())]).is_err());
    assert_eq!(heap.array_snapshot(invalid).unwrap(), vec![Value::I32(1)]);
    assert!(
        array_join(
            &GcHeap::new(
                GcHeapConfig::default(),
                std::rc::Rc::new(crate::resource::ResourceState::default())
            ),
            &[Value::Array(handle), Value::Str("".into())]
        )
        .is_err()
    );
}
