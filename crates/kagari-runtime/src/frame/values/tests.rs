use super::*;
use crate::Runtime;
use kagari_bytecode::{
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
        .allocate(2, 1, &arguments, module.clone(), None)
        .unwrap();
    let other = foreign
        .allocate(2, 1, &arguments, module.clone(), None)
        .unwrap();
    assert!(values.get(other).is_none());
    assert!(values.release(other).is_none());
    values.release(first).unwrap();
    let second = values.allocate(2, 1, &arguments, module, None).unwrap();
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
        )
        .unwrap();
    let second = values
        .allocate(1, 0, &FrameArguments::plain(&[Value::I32(2)]), module, None)
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
