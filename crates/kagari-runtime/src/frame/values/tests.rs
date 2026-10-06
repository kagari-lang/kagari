use super::*;
use crate::Runtime;
use crate::module::execution::layout::Location;
use kagari_bytecode::{
    instruction::Register,
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_types::scalar::BuiltinType;
use std::mem::size_of;

fn module() -> (Runtime, LoadedModule) {
    let mut runtime = Runtime::default();
    let module = runtime
        .load_program(
            "windows",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    (runtime, module)
}

#[test]
fn frame_windows_reject_foreign_and_reused_identities() {
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    let mut foreign = ExecutionValues::default();
    let arguments = FrameArguments::plain(&[Value::I32(42)]);
    let first = values
        .allocate(2, 1, &arguments, module.clone(), None, None)
        .unwrap();
    let other = foreign
        .allocate(2, 1, &arguments, module.clone(), None, None)
        .unwrap();
    assert!(values.get(other).is_none());
    assert!(values.release(other).is_none());
    values.release(first).unwrap();
    let second = values
        .allocate(2, 1, &arguments, module, None, None)
        .unwrap();
    assert_eq!(first.index, second.index);
    assert!(values.get(first).is_none());
    assert!(values.ranges(first).is_none());
    assert!(values.release(first).is_none());
    assert_eq!(values.get(second).unwrap(), &[Value::Unit, Value::I32(42)]);
}

#[test]
fn retiring_an_outer_window_does_not_unroot_a_suspended_inner_window() {
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    let first = values
        .allocate(
            1,
            0,
            &FrameArguments::plain(&[Value::I32(1)]),
            module.clone(),
            None,
            None,
        )
        .unwrap();
    let second = values
        .allocate(
            1,
            0,
            &FrameArguments::plain(&[Value::I32(2)]),
            module,
            None,
            None,
        )
        .unwrap();
    values.release(first).unwrap();
    assert_eq!(values.get(second).unwrap(), &[Value::I32(2)]);
    let mut roots = Vec::new();
    values.append_values(&mut roots);
    assert_eq!(roots, [Value::Unit, Value::I32(2)]);
    let mut metadata = Vec::new();
    values.append_metadata(&mut metadata);
    assert_eq!(metadata.len(), 1);
    values.release(second).unwrap();
    roots.clear();
    metadata.clear();
    values.append_values(&mut roots);
    values.append_metadata(&mut metadata);
    assert!(roots.is_empty());
    assert!(metadata.is_empty());
}

#[test]
fn register_arguments_survive_growth_reordering_and_repeated_sources() {
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    let caller = values
        .allocate(
            2,
            0,
            &FrameArguments::plain(&[Value::I64(11), Value::I64(23)]),
            module.clone(),
            None,
            None,
        )
        .unwrap();
    let registers = [Register::new(1), Register::new(0), Register::new(1)];
    let arguments = FrameArguments::frame(caller, &registers);
    let callee = values
        .allocate(8192, 4096, &arguments, module.clone(), None, None)
        .unwrap();
    assert_eq!(
        &values.get(callee).unwrap()[4096..4099],
        &[Value::I64(23), Value::I64(11), Value::I64(23)]
    );
    assert_eq!(
        values.get(caller).unwrap(),
        &[Value::I64(11), Value::I64(23)]
    );
    values.release(callee).unwrap();
    values.release(caller).unwrap();
    assert!(
        values
            .allocate(3, 0, &arguments, module, None, None)
            .is_err()
    );
}

fn scalar_layout() -> Arc<FrameLayout> {
    Arc::new(FrameLayout {
        locations: vec![
            Location {
                operand: OperandSlot::new(0, false),
                representation: ValueType::U64,
                semantic: Some(BuiltinType::U64),
            },
            Location {
                operand: OperandSlot::new(1, false),
                representation: ValueType::F64,
                semantic: Some(BuiltinType::F64),
            },
            Location {
                operand: OperandSlot::new(2, false),
                representation: ValueType::I32,
                semantic: Some(BuiltinType::I8),
            },
            Location {
                operand: OperandSlot::new(0, true),
                representation: ValueType::Str,
                semantic: Some(BuiltinType::String),
            },
        ]
        .into(),
        register_count: 4,
        count: 4,
        scalar_count: 3,
        managed_count: 1,
    })
}

#[test]
fn scalar_windows_preserve_bits_initialization_and_managed_roots() {
    println!(
        "STORAGE_LAYOUT,payload_bytes={},initialization_bytes={},location_bytes={},window_bytes={}",
        size_of::<u64>(),
        size_of::<bool>(),
        size_of::<Location>(),
        size_of::<Window>()
    );
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    let slots = values
        .allocate(
            4,
            0,
            &FrameArguments::plain(&[]),
            module,
            None,
            Some(scalar_layout()),
        )
        .unwrap();
    let ranges = values.ranges(slots).unwrap();
    assert!(
        values
            .payload(&ranges, OperandSlot::new(0, false).scalar().unwrap())
            .is_none()
    );
    assert_eq!(values.with_value(slots, 0, Value::clone), Some(Value::Unit));
    values.set(slots, 0, Value::U64(u64::MAX)).unwrap();
    for bits in [0x8000_0000_0000_0000, 0x7ff0_0000_0000_0001, u64::MAX] {
        values
            .set(slots, 1, Value::F64(f64::from_bits(bits)))
            .unwrap();
        let Some(Value::F64(v)) = values.with_value(slots, 1, Value::clone) else {
            panic!()
        };
        assert_eq!(v.to_bits(), bits);
    }
    assert!(values.set(slots, 2, Value::I32(128)).is_none());
    assert!(values.set(slots, 2, Value::I64(1)).is_none());
    values.set(slots, 2, Value::I32(-128)).unwrap();
    values.set(slots, 3, Value::Str("root".into())).unwrap();
    let mut roots = Vec::new();
    values.append_values(&mut roots);
    assert_eq!(roots, [Value::Str("root".into())]);
    values.release(slots).unwrap();
    assert!(values.payloads.is_empty());
    assert!(values.initialized.is_empty());
    assert!(values.values.is_empty());
    assert!(values.with_value(slots, 0, Value::clone).is_none());
}

#[test]
fn invalid_scalar_admission_never_publishes_a_partial_frame() {
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    assert!(
        values
            .allocate(
                4,
                2,
                &FrameArguments::plain(&[Value::I32(128)]),
                module.clone(),
                None,
                Some(scalar_layout())
            )
            .is_err()
    );
    assert_eq!(values.active_windows(), 0);
    assert!(values.payloads.is_empty());
    let slots = values
        .allocate(
            4,
            0,
            &FrameArguments::plain(&[Value::U64(u64::MAX)]),
            module,
            None,
            Some(scalar_layout()),
        )
        .unwrap();
    assert_eq!(
        values.with_value(slots, 0, Value::clone),
        Some(Value::U64(u64::MAX))
    );
}
