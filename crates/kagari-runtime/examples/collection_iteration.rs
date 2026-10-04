//! Run with `cargo run -p kagari-runtime --example collection_iteration`.
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_runtime::{Runtime, error::RuntimeErrorKind, value::Value};
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn main() {
    let mut runtime = Runtime::default();
    let owner = runtime
        .load_program(
            "arrays",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();

    let gc = runtime.gc();
    let id = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
    let value = Value::Array(id);
    let iteration = gc.begin_collection_iteration(&value).unwrap();
    // Aliases may replace an existing element, but cannot change structure.
    gc.array_set(id, 0, Value::I32(42)).unwrap();
    assert_eq!(
        gc.array_set(id, 1, Value::I32(9)).unwrap_err().kind(),
        RuntimeErrorKind::IndexOutOfBounds,
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
