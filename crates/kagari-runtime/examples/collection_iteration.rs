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
    assert!(gc.array_push(id, Value::I32(2)).is_err());
    assert_eq!(gc.array_len(id), Some(1));
    drop(iteration);
    gc.array_push(id, Value::I32(2)).unwrap();
    assert_eq!(gc.array_len(id), Some(2));
}
