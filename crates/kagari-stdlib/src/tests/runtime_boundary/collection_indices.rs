//! Native stdlib contracts keep wide script indices until the storage boundary.
use crate::collections::vector::VecValues;
use crate::tests::runtime_boundary::support::{compile_program, native::invoke::invoke};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    module::ExecutableModule,
    native::{conversion::context::ConversionContext, module::NativeModule},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::collections::BTreeMap;

fn fixture() -> (Runtime, ExecutableModule, BTreeMap<String, usize>) {
    let modules = crate::modules().unwrap();
    let program = compile_program(
        &modules,
        "collection_indices.kgr",
        r#"
fn get(values: Vec<u64>, index: u64) -> Option<u64> { values.get(index) }
fn remove(values: Vec<u64>, index: u64) -> Option<u64> { values.remove(index) }
fn insert(values: Vec<u64>, index: u64) -> Vec<u64> { values.insert(index, 42u64) }
fn insert_mutable<T: std::collections::MutableList<u64>>(values: T, index: u64) {
    values.insert(index, 42u64);
}
fn insert_unit(values: Vec<u64>, index: u64) { insert_mutable(values, index); }
fn array_index(values: [u64], index: u64) -> u64 { values.index(index) }
fn slice(value: String, start: u64, end: u64) -> String { value.slice(start, end) }
"#,
    );
    let slots = program.records().modules[program.records().root.index()]
        .native_imports
        .iter()
        .enumerate()
        .map(|(slot, import)| {
            let name = program.definitions().resolve(import.binding).unwrap();
            let name = name.last().unwrap().name;
            let key =
                if name == "insert" && import.signature.result == Ty::Builtin(BuiltinType::Unit) {
                    "insert_unit"
                } else {
                    name
                };
            (key.to_owned(), slot)
        })
        .collect();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner = runtime.load_program("collection_indices", program).unwrap();
    let _owner_program = owner;
    let owner = runtime.select_program(&_owner_program).unwrap();
    (runtime, owner, slots)
}

#[test]
fn wide_collection_indices_preserve_absence_traps_and_mutation() {
    let (runtime, owner, slots) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let values = cx.encode(VecValues(vec![1u64 << 32])).unwrap();
    let value = values.value(runtime.gc()).unwrap();
    let array = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::U64),
            vec![Value::U64(1 << 32)],
        )
        .unwrap();
    let array = runtime.root_value(Value::Array(array)).unwrap();
    for index in [1, 1u64 << 32, u64::MAX] {
        for name in ["get", "remove"] {
            let result =
                invoke(&runtime, &owner, slots[name], &[value, Value::U64(index)]).unwrap();
            let result = runtime.root_value(result).unwrap();
            assert_eq!(
                cx.decode::<Option<u64>>(&result).unwrap(),
                None,
                "{name}: {index}"
            );
        }
        let error = invoke(
            &runtime,
            &owner,
            slots["index"],
            &[array.value(runtime.gc()).unwrap(), Value::U64(index)],
        )
        .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::IndexOutOfBounds);
        assert!(error.to_string().contains(&index.to_string()));
    }
    for index in [2, 1u64 << 32, u64::MAX] {
        for name in ["insert_unit", "insert"] {
            let error = invoke(
                &runtime,
                &owner,
                slots[name],
                &[value, Value::U64(index), Value::U64(42)],
            )
            .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::IndexOutOfBounds);
            assert!(error.to_string().contains(&index.to_string()));
            assert_eq!(
                cx.decode::<VecValues<u64>>(&values).unwrap().0,
                [1u64 << 32]
            );
        }
    }
    let result = invoke(
        &runtime,
        &owner,
        slots["insert_unit"],
        &[value, Value::U64(1), Value::U64(42)],
    )
    .unwrap();
    assert_eq!(result, Value::Unit);
    let result = invoke(
        &runtime,
        &owner,
        slots["insert"],
        &[value, Value::U64(2), Value::U64(43)],
    )
    .unwrap();
    assert_eq!(result, value);
    assert_eq!(
        cx.decode::<VecValues<u64>>(&values).unwrap().0,
        [1u64 << 32, 42, 43]
    );
    let result = invoke(&runtime, &owner, slots["remove"], &[value, Value::U64(0)]).unwrap();
    let result = runtime.root_value(result).unwrap();
    assert_eq!(cx.decode::<Option<u64>>(&result).unwrap(), Some(1u64 << 32));
    assert_eq!(cx.decode::<VecValues<u64>>(&values).unwrap().0, [42, 43]);
}

#[test]
fn string_slice_preserves_wide_range_and_utf8_errors() {
    let (runtime, owner, slots) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let text = cx.encode("a火".to_owned()).unwrap();
    let value = text.value(runtime.gc()).unwrap();
    let slot = slots["slice"];
    for (start, end) in [
        (0, 1u64 << 32),
        (0, u64::MAX),
        (u64::MAX, u64::MAX),
        (3, 1),
        (1, 2),
    ] {
        let error = invoke(
            &runtime,
            &owner,
            slot,
            &[value, Value::U64(start), Value::U64(end)],
        )
        .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::IndexOutOfBounds);
        assert!(error.to_string().contains(&format!("{start}..{end}")));
    }
    for (start, end, expected) in [(1, 4, "火"), (4, 4, "")] {
        let result = invoke(
            &runtime,
            &owner,
            slot,
            &[value, Value::U64(start), Value::U64(end)],
        )
        .unwrap();
        let result = runtime.root_value(result).unwrap();
        assert_eq!(cx.decode::<String>(&result).unwrap(), expected);
    }
}
