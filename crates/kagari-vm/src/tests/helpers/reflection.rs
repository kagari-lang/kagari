use super::*;
use crate::tests::{common, common::standard_runtime};
use kagari_bytecode::program::{BytecodeProgram, ModuleRef};
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn executes_runtime_reflect_type_of_helper() {
    let (runtime, loaded) = load_reflection_bytecode_module(
        "reflect_type.kbc",
        test_function_module(
            "main",
            vec![
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
            ValueType::Str,
            vec![ValueType::I32, ValueType::Str],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::Str("i32".to_owned()));
}

#[test]
fn runtime_reflection_helpers_use_declared_metadata() {
    let (runtime, loaded) = load_bytecode_module(
        "reflect_denied.kbc",
        test_function_module(
            "main",
            vec![
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
            ValueType::Str,
            vec![ValueType::I32, ValueType::Str],
        ),
    );

    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::Str("i32".into())
    );
}

#[test]
fn declared_reflection_reads_are_available() {
    let mut metadata_only = standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    let loaded = metadata_only
        .load_program(
            "reflect_read_denied.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![common::point_function_module(
                    "main",
                    vec![
                        BytecodeInstruction::LoadConst {
                            dst: Register::new(0),
                            constant: ConstantOperand::I32(1),
                        },
                        BytecodeInstruction::MakeStruct {
                            dst: Register::new(1),
                            structure: StructId::new(0),
                            arguments: vec![],
                            fields: vec![Register::new(0)],
                        },
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(2)),
                            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField(
                                "x".to_owned(),
                            )),
                            args: vec![Register::new(1)],
                        },
                        BytecodeInstruction::Return(Some(Register::new(2))),
                    ],
                    ValueType::I32,
                    vec![ValueType::I32, ValueType::HeapObject, ValueType::I32],
                )],
            },
        )
        .unwrap();
    let mut vm = Vm::new(metadata_only);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(1)
    );
}

#[test]
fn declared_reflection_writes_are_available() {
    let mut read_only = standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    let loaded = read_only
        .load_program(
            "reflect_write_denied.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![common::point_function_module(
                    "main",
                    vec![
                        BytecodeInstruction::LoadConst {
                            dst: Register::new(0),
                            constant: ConstantOperand::I32(1),
                        },
                        BytecodeInstruction::MakeStruct {
                            dst: Register::new(1),
                            structure: StructId::new(0),
                            arguments: vec![],
                            fields: vec![Register::new(0)],
                        },
                        BytecodeInstruction::LoadConst {
                            dst: Register::new(2),
                            constant: ConstantOperand::I32(2),
                        },
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(3)),
                            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(
                                "x".to_owned(),
                            )),
                            args: vec![Register::new(1), Register::new(2)],
                        },
                        BytecodeInstruction::Return(Some(Register::new(3))),
                    ],
                    ValueType::HeapObject,
                    vec![
                        ValueType::I32,
                        ValueType::HeapObject,
                        ValueType::I32,
                        ValueType::HeapObject,
                    ],
                )],
            },
        )
        .unwrap();
    let mut vm = Vm::new(read_only);
    vm.execute(&loaded, "main").unwrap();
}

#[test]
fn executes_runtime_reflect_get_and_set_field_helpers() {
    let (runtime, loaded) = load_reflection_bytecode_module(
        "reflect_field.kbc",
        common::point_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(1),
                },
                BytecodeInstruction::MakeStruct {
                    dst: Register::new(1),
                    structure: StructId::new(0),
                    arguments: vec![],
                    fields: vec![Register::new(0)],
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
            ValueType::I32,
            vec![
                ValueType::I32,
                ValueType::HeapObject,
                ValueType::I32,
                ValueType::HeapObject,
                ValueType::I32,
            ],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(9));
}

#[test]
fn executes_runtime_reflect_set_index_helper() {
    let (runtime, loaded) = load_reflection_bytecode_module(
        "reflect_index.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(1),
                },
                BytecodeInstruction::LoadConst {
                    dst: Register::new(1),
                    constant: ConstantOperand::I32(2),
                },
                BytecodeInstruction::MakeArray {
                    element: Ty::Builtin(BuiltinType::I32),
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
            ValueType::HeapObject,
            vec![
                ValueType::I32,
                ValueType::I32,
                ValueType::HeapObject,
                ValueType::I32,
                ValueType::HeapObject,
            ],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    let Value::Array(handle) = report.return_value else {
        panic!("expected array return value");
    };
    assert_eq!(
        vm.runtime().gc().array_snapshot(handle),
        Some(vec![Value::I32(2), Value::I32(2)])
    );
}

#[test]
fn executes_source_lowered_type_of_helper() {
    let (runtime, loaded) = load_reflection_test_module("fn main() -> String { type_of(7) }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::Str("i32".to_owned()));
}

#[test]
fn executes_source_lowered_reflection_field_helpers() {
    let (runtime, loaded) = load_reflection_test_module(
        r#"
struct Point { var x: i32 }

fn main() -> i32 {
    val point = Point { x: 1 };
    val next = set_field(point, "x", 9);
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
    let (runtime, loaded) = load_reflection_test_module(
        r#"
fn main() -> Vec<i32> {
    val values = [1, 2];
    set_index(values, 0, 9)
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    let Value::Array(handle) = report.return_value else {
        panic!("expected array return value");
    };
    assert_eq!(
        vm.runtime().gc().array_snapshot(handle),
        Some(vec![Value::I32(9), Value::I32(2)])
    );
}

#[test]
fn executes_source_lowered_place_assignments() {
    let (runtime, loaded) = load_test_module(
        r#"
struct Point { var x: i32 }
struct Holder { var inner: Point }

fn main() -> i32 {
    var holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 7;
    var values = [1, 2];
    values[0] = 5;
    holder.inner.x + values[0]
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(12));
}

#[test]
fn standard_collection_reflection_metadata_reports_runtime_categories() {
    let (runtime, loaded) = load_reflection_test_module(
        r#"use std::collections::{HashMap, HashSet};

        fn main() -> (String, String) {
            val map: HashMap<i32,i32> = HashMap::new();
            val set: HashSet<i32> = HashSet::new();
            (type_of(map), type_of(set))
        }
    "#,
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![
            Value::Str("map".to_owned()),
            Value::Str("set".to_owned())
        ])
    );
}
