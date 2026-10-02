use super::*;
use crate::{error::VmError, tests::common};
use kagari_abi::{scalar::BuiltinType, types::AbiType};
use kagari_bytecode::program::{BytecodeProgram, ModuleRef};

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
fn runtime_reflection_helpers_require_runtime_capability() {
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
    let error = vm.execute(&loaded, "main").unwrap_err();

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("reflection_metadata")
    ));
}

#[test]
fn reflection_metadata_and_read_gates_are_separate() {
    let mut metadata_only = Runtime::new(RuntimeConfig {
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
    let error = vm.execute(&loaded, "main").unwrap_err();

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("reflection_read")
    ));
}

#[test]
fn reflection_read_and_write_gates_are_separate() {
    let mut read_only = Runtime::new(RuntimeConfig {
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
    let error = vm.execute(&loaded, "main").unwrap_err();

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("reflection_write")
    ));
}

#[test]
fn reflection_helpers_enforce_reflection_operation_resource_limit() {
    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            max_reflection_operations: Some(1),
            ..RuntimeLimits::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program(
            "reflect_operation_limit.kbc",
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
                            fields: vec![Register::new(0)],
                        },
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(2)),
                            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
                            args: vec![Register::new(1)],
                        },
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(3)),
                            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField(
                                "x".to_owned(),
                            )),
                            args: vec![Register::new(1)],
                        },
                        BytecodeInstruction::Return(Some(Register::new(3))),
                    ],
                    ValueType::I32,
                    vec![
                        ValueType::I32,
                        ValueType::HeapObject,
                        ValueType::Str,
                        ValueType::I32,
                    ],
                )],
            },
        )
        .unwrap();
    let mut vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
                && error.message().contains("reflection operations")
    ));
    assert_eq!(vm.runtime().resources().counters().reflection_operations, 1);
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
                    element: AbiType::Builtin(BuiltinType::I32),
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
fn main() -> ArrayList<i32> {
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
        r#"
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
