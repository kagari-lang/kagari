use super::*;
use crate::Runtime;
use kagari_bytecode::{
    instruction::Register,
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};

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
    assert!(values.get_mut(first).is_none());
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
