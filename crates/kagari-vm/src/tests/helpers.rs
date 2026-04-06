use kagari_ir::bytecode::{
    BytecodeFunction, BytecodeInstruction, BytecodeModule, CallTarget, ConstantOperand,
    FunctionRef, Register, RuntimeHelper, StructFieldInit,
};
use kagari_runtime::{
    Runtime,
    host::{HostError, HostFunction},
    value::Value,
};

use crate::Vm;
use crate::tests::common::{load_bytecode_module, load_test_module};

#[test]
fn executes_runtime_host_helper_call() {
    let mut runtime = Runtime::default();
    runtime.host_mut().register(HostFunction::new(
        "host.add_i32",
        vec![],
        "i32",
        |args| match args {
            [Value::I32(lhs), Value::I32(rhs)] => Ok(Value::I32(lhs + rhs)),
            _ => Err(HostError::new("host.add_i32 expects two i32 arguments")),
        },
    ));

    let (_, loaded) = load_bytecode_module(
        "helper.kbc",
        BytecodeModule {
            module_init: None,
            module_slots: vec![],
            functions: vec![BytecodeFunction {
                id: FunctionRef::new(0),
                name: "main".to_owned(),
                parameter_count: 0,
                register_count: 3,
                local_count: 0,
                instructions: vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantOperand::I32(40),
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(1),
                        constant: ConstantOperand::I32(2),
                    },
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(2)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                            "host.add_i32".to_owned(),
                        )),
                        args: vec![Register::new(0), Register::new(1)],
                    },
                    BytecodeInstruction::Return(Some(Register::new(2))),
                ],
            }],
        },
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(42));
}

#[test]
fn executes_runtime_reflect_type_of_helper() {
    let (runtime, loaded) = load_bytecode_module(
        "reflect_type.kbc",
        BytecodeModule {
            module_init: None,
            module_slots: vec![],
            functions: vec![BytecodeFunction {
                id: FunctionRef::new(0),
                name: "main".to_owned(),
                parameter_count: 0,
                register_count: 2,
                local_count: 0,
                instructions: vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantOperand::I32(7),
                    },
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(1)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
                        args: vec![Register::new(0)],
                    },
                    BytecodeInstruction::Return(Some(Register::new(1))),
                ],
            }],
        },
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::Str("i32".to_owned()));
}

#[test]
fn executes_runtime_reflect_get_and_set_field_helpers() {
    let (runtime, loaded) = load_bytecode_module(
        "reflect_field.kbc",
        BytecodeModule {
            module_init: None,
            module_slots: vec![],
            functions: vec![BytecodeFunction {
                id: FunctionRef::new(0),
                name: "main".to_owned(),
                parameter_count: 0,
                register_count: 5,
                local_count: 0,
                instructions: vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantOperand::I32(1),
                    },
                    BytecodeInstruction::MakeStruct {
                        dst: Register::new(1),
                        name: "Point".to_owned(),
                        fields: vec![StructFieldInit {
                            name: "x".to_owned(),
                            value: Register::new(0),
                        }],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(2),
                        constant: ConstantOperand::I32(9),
                    },
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(3)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(
                            "x".to_owned(),
                        )),
                        args: vec![Register::new(1), Register::new(2)],
                    },
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(4)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField(
                            "x".to_owned(),
                        )),
                        args: vec![Register::new(3)],
                    },
                    BytecodeInstruction::Return(Some(Register::new(4))),
                ],
            }],
        },
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(9));
}

#[test]
fn executes_runtime_reflect_set_index_helper() {
    let (runtime, loaded) = load_bytecode_module(
        "reflect_index.kbc",
        BytecodeModule {
            module_init: None,
            module_slots: vec![],
            functions: vec![BytecodeFunction {
                id: FunctionRef::new(0),
                name: "main".to_owned(),
                parameter_count: 0,
                register_count: 5,
                local_count: 0,
                instructions: vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantOperand::I32(1),
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(1),
                        constant: ConstantOperand::I32(2),
                    },
                    BytecodeInstruction::MakeArray {
                        dst: Register::new(2),
                        elements: vec![Register::new(0), Register::new(1)],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(3),
                        constant: ConstantOperand::I32(0),
                    },
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(4)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
                        args: vec![Register::new(2), Register::new(3), Register::new(1)],
                    },
                    BytecodeInstruction::Return(Some(Register::new(4))),
                ],
            }],
        },
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Array(vec![Value::I32(2), Value::I32(2)])
    );
}

#[test]
fn executes_source_lowered_type_of_helper() {
    let (runtime, loaded) = load_test_module("fn main() -> str { type_of(7) }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::Str("i32".to_owned()));
}

#[test]
fn executes_source_lowered_reflection_field_helpers() {
    let (runtime, loaded) = load_test_module(
        r#"
struct Point { x: i32 }

fn main() -> i32 {
    let point = Point { x: 1 };
    let next = set_field(point, "x", 9);
    get_field(next, "x")
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(9));
}

#[test]
fn executes_source_lowered_set_index_helper() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> [i32] {
    let values = [1, 2];
    set_index(values, 0, 9)
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Array(vec![Value::I32(9), Value::I32(2)])
    );
}

#[test]
fn executes_source_lowered_place_assignments() {
    let (runtime, loaded) = load_test_module(
        r#"
struct Point { x: i32 }
struct Holder { inner: Point }

fn main() -> i32 {
    let mut holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 7;
    let mut values = [1, 2];
    values[0] = 5;
    holder.inner.x + values[0]
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(12));
}
