//! Run with `cargo run -p kagari-runtime --example collection_iteration`.
use kagari_runtime::{Runtime, value::Value};

fn main() {
    let runtime = Runtime::default();
    let gc = runtime.gc();
    let id = gc.alloc_array(vec![Value::I32(1)]).unwrap();
    let value = Value::Array(id);
    let iteration = gc.begin_collection_iteration(&value).unwrap();
    // Aliases may replace an existing element, but cannot change structure.
    gc.array_set(id, 0, Value::I32(42)).unwrap();
    assert_eq!(
        gc.array_set(id, 1, Value::I32(9)).unwrap_err().kind(),
        kagari_runtime::RuntimeErrorKind::IndexOutOfBounds,
    );
    assert_eq!(gc.array_get(id, 0), Some(Value::I32(42)));
    assert!(gc.array_push(id, Value::I32(2)).is_err());
    assert_eq!(gc.array_len(id), Some(1));
    assert!(gc.array_pop(id).is_err());
    drop(iteration);
    gc.array_push(id, Value::I32(2)).unwrap();
    assert_eq!(gc.array_len(id), Some(2));
    assert_eq!(gc.array_pop(id).unwrap(), Some(Value::I32(2)));
    gc.array_clear(id).unwrap();
    assert_eq!(gc.array_pop(id).unwrap(), None);
}
