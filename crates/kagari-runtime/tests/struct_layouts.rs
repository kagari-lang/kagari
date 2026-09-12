#[path = "support/layouts.rs"]
mod layouts;

use kagari_ir::{bytecode::StructId, module::ValueType};
use kagari_runtime::{Runtime, RuntimeErrorKind, reflection, value::Value};

#[test]
fn slot_access_checks_nominal_owner_schema_permission_and_representation() {
    let mut runtime = Runtime::default();
    let layout = layouts::layout(
        &mut runtime,
        "Point",
        &[
            ("x", ValueType::I32, true),
            ("fixed", ValueType::Bool, false),
        ],
    );
    let object = runtime
        .alloc_struct(layout.clone(), vec![Value::I32(1), Value::Bool(true)])
        .unwrap();
    let mut foreign = layout.module().bytecode.clone();
    foreign.structures[0].declaration.module.path = vec!["other.kgr".into()];
    for field in &mut foreign.structures[0].fields {
        field.declaration.module.path = vec!["other.kgr".into()];
    }
    let foreign = runtime
        .load_module("other", foreign)
        .unwrap()
        .struct_layout(StructId::new(0))
        .unwrap();
    let heap = runtime.gc();
    assert!(heap.struct_get_slot(object, &foreign, 0).is_none());
    assert!(
        heap.struct_set_slot(object, &foreign, 0, Value::I32(99))
            .is_none()
    );
    assert!(
        heap.struct_set_slot(object, &layout, 1, Value::Bool(false))
            .is_none()
    );
    assert!(
        heap.struct_set_slot(object, &layout, 0, Value::Bool(false))
            .is_none()
    );
    assert!(
        heap.struct_set_slot(object, &layout, usize::MAX, Value::I32(99))
            .is_none()
    );
    assert!(
        reflection::set_field(heap, &Value::Struct(object), "fixed", Value::Bool(false)).is_err()
    );
    assert!(reflection::set_field(heap, &Value::Struct(object), "x", Value::Bool(false)).is_err());
    assert_eq!(
        heap.struct_get_slot(object, &layout, 0),
        Some(Value::I32(1))
    );
    assert_eq!(
        heap.struct_get_slot(object, &layout, 1),
        Some(Value::Bool(true))
    );
    reflection::set_field(heap, &Value::Struct(object), "x", Value::I32(42)).unwrap();
    assert_eq!(
        heap.struct_get_slot(object, &layout, 0),
        Some(Value::I32(42))
    );
}

#[test]
fn allocation_rejects_foreign_layout_and_invalid_initializers_before_accounting() {
    let mut runtime = Runtime::default();
    let layout = layouts::layout(&mut runtime, "Point", &[("x", ValueType::I32, true)]);
    let mut other = Runtime::default();
    let foreign = layouts::layout(&mut other, "Point", &[("x", ValueType::I32, true)]);
    let counters = runtime.resources().counters();
    assert_eq!(
        runtime
            .alloc_struct(foreign.clone(), vec![Value::I32(7)])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    for fields in [
        vec![],
        vec![Value::Bool(false)],
        vec![Value::I32(1), Value::I32(2)],
    ] {
        assert_eq!(
            runtime
                .alloc_struct(layout.clone(), fields)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
    assert_eq!(runtime.resources().counters(), counters);
    assert_eq!(runtime.gc().allocated_objects(), 0);
    let object = runtime.alloc_struct(layout, vec![Value::I32(1)]).unwrap();
    assert!(runtime.gc().struct_get_slot(object, &foreign, 0).is_none());
    assert!(
        runtime
            .gc()
            .struct_set_slot(object, &foreign, 0, Value::I32(2))
            .is_none()
    );
}

#[test]
fn objects_retain_old_layouts_and_require_equal_schemas_across_generations() {
    let mut runtime = Runtime::default();
    let original = layouts::layout(&mut runtime, "Point", &[("x", ValueType::I32, true)]);
    let object = runtime
        .alloc_struct(original.clone(), vec![Value::I32(1)])
        .unwrap();
    let next = runtime
        .reload_module(
            original.module(),
            "Point",
            original.module().bytecode.clone(),
        )
        .unwrap();
    let compatible = next.struct_layout(StructId::new(0)).unwrap();
    assert_ne!(original.module().key(), compatible.module().key());
    runtime
        .gc()
        .struct_set_slot(object, &compatible, 0, Value::I32(42))
        .unwrap();
    let original_key = original.module().key();
    drop(original);
    assert!(
        runtime
            .modules()
            .collect_unreachable_epochs()
            .contains(&original_key)
    );
    let retained = runtime.gc().struct_layout(object).unwrap();
    assert_eq!(retained.module().key(), original_key);
    assert_eq!(
        reflection::get_field(runtime.gc(), &Value::Struct(object), "x").unwrap(),
        Value::I32(42)
    );
    // A retained verified generation can still allocate without a module-store lookup.
    assert!(
        runtime
            .alloc_struct(retained.clone(), vec![Value::I32(3)])
            .is_ok()
    );
    let changed = layouts::layout(&mut runtime, "Point", &[("x", ValueType::I32, false)]);
    assert!(runtime.gc().struct_get_slot(object, &changed, 0).is_none());
    assert!(
        runtime
            .gc()
            .struct_set_slot(object, &changed, 0, Value::I32(99))
            .is_none()
    );
    assert_eq!(
        runtime.gc().struct_get_slot(object, &retained, 0),
        Some(Value::I32(42))
    );
}
