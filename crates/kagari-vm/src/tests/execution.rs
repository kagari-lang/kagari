use std::sync::{Arc, Mutex};

use kagari_ir::bytecode::{
    BytecodeFunction, BytecodeInstruction, BytecodeModule, CallTarget, ConstantOperand,
    FunctionRef, Register, RuntimeHelper,
};
use kagari_runtime::Runtime;
use kagari_runtime::host::HostFunction;
use kagari_runtime::value::{StructValueField, Value};

use crate::tests::common::{load_bytecode_module, load_test_module};
use crate::{Vm, VmError};

#[test]
fn executes_simple_arithmetic_function() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { let value = 1 + 2; value }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(3));
}

#[test]
fn executes_if_control_flow() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { if true { 1 } else { 2 } }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(1));
}

#[test]
fn executes_direct_function_calls() {
    let (runtime, loaded) = load_test_module(
        r#"
fn callee() -> i32 { 7 }
fn main() -> i32 { callee() }
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(7));
}

#[test]
fn executes_array_index_access() {
    let (runtime, loaded) =
        load_test_module("fn main() -> i32 { let values = [1, 2, 3]; values[1] }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(2));
}

#[test]
fn executes_struct_field_access() {
    let (runtime, loaded) = load_test_module(
        r#"
struct Point { x: i32, y: i32 }

fn main() -> i32 {
    let point = Point { x: 1, y: 2 };
    point.y
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(2));
}

#[test]
fn executes_tuple_literal_return() {
    let (runtime, loaded) = load_test_module("fn main() -> (bool, bool) { (true, false) }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![Value::Bool(true), Value::Bool(false)])
    );
}

#[test]
fn executes_struct_literal_return() {
    let (runtime, loaded) = load_test_module(
        r#"
struct Point { x: i32, y: i32 }

fn main() -> Point {
    Point { x: 1, y: 2 }
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Struct {
            name: "Point".to_owned(),
            fields: vec![
                StructValueField {
                    name: "x".to_owned(),
                    value: Value::I32(1),
                },
                StructValueField {
                    name: "y".to_owned(),
                    value: Value::I32(2),
                },
            ],
        }
    );
}

#[test]
fn executes_top_level_tail_expression_as_module_result() {
    let (runtime, loaded) = load_test_module(
        r#"
let value = 1;

value + 2
"#,
    );
    let mut vm = Vm::new(runtime);
    let result = vm
        .execute_module(&loaded)
        .expect("module init should execute");

    assert_eq!(result, Value::I32(3));
}

#[test]
fn executes_module_init_before_entry_only_once_per_module_epoch() {
    let init_count = Arc::new(Mutex::new(0usize));
    let counter = Arc::clone(&init_count);

    let mut runtime = Runtime::default();
    runtime.host_mut().register(HostFunction::new(
        "host.bump_init",
        vec![],
        "unit",
        move |_| {
            let mut count = counter.lock().expect("counter lock should succeed");
            *count += 1;
            Ok(Value::Unit)
        },
    ));

    let (_, loaded) = load_bytecode_module(
        "module_init_once.kgr",
        BytecodeModule {
            module_init: Some(FunctionRef::new(0)),
            module_slots: vec![],
            functions: vec![
                BytecodeFunction {
                    id: FunctionRef::new(0),
                    name: "__module_init__".to_owned(),
                    parameter_count: 0,
                    register_count: 0,
                    local_count: 0,
                    instructions: vec![
                        BytecodeInstruction::Call {
                            dst: None,
                            callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                                "host.bump_init".to_owned(),
                            )),
                            args: vec![],
                        },
                        BytecodeInstruction::Return(None),
                    ],
                },
                BytecodeFunction {
                    id: FunctionRef::new(1),
                    name: "main".to_owned(),
                    parameter_count: 0,
                    register_count: 1,
                    local_count: 0,
                    instructions: vec![
                        BytecodeInstruction::LoadConst {
                            dst: Register::new(0),
                            constant: ConstantOperand::I32(7),
                        },
                        BytecodeInstruction::Return(Some(Register::new(0))),
                    ],
                },
            ],
        },
    );

    let mut vm = Vm::new(runtime);
    let first = vm
        .execute(&loaded, "main")
        .expect("first execution should work");
    let second = vm
        .execute(&loaded, "main")
        .expect("second execution should work");

    assert_eq!(first.return_value, Value::I32(7));
    assert_eq!(second.return_value, Value::I32(7));
    assert_eq!(*init_count.lock().expect("counter lock should succeed"), 1);
}

#[test]
fn reruns_module_init_for_new_module_epoch() {
    let init_count = Arc::new(Mutex::new(0usize));
    let counter = Arc::clone(&init_count);

    let mut runtime = Runtime::default();
    runtime.host_mut().register(HostFunction::new(
        "host.bump_init",
        vec![],
        "unit",
        move |_| {
            let mut count = counter.lock().expect("counter lock should succeed");
            *count += 1;
            Ok(Value::Unit)
        },
    ));

    let bytecode = BytecodeModule {
        module_init: Some(FunctionRef::new(0)),
        module_slots: vec![],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: "__module_init__".to_owned(),
            parameter_count: 0,
            register_count: 0,
            local_count: 0,
            instructions: vec![
                BytecodeInstruction::Call {
                    dst: None,
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                        "host.bump_init".to_owned(),
                    )),
                    args: vec![],
                },
                BytecodeInstruction::Return(None),
            ],
        }],
    };
    let first_loaded = runtime.load_module("reloadable.kgr", bytecode.clone());
    let second_loaded = runtime.load_module("reloadable.kgr", bytecode);

    let mut vm = Vm::new(runtime);
    vm.execute_module(&first_loaded)
        .expect("first module epoch should initialize");
    vm.execute_module(&second_loaded)
        .expect("second module epoch should initialize");

    assert_eq!(*init_count.lock().expect("counter lock should succeed"), 2);
}

#[test]
fn caches_failed_module_init_without_retrying() {
    let init_count = Arc::new(Mutex::new(0usize));
    let counter = Arc::clone(&init_count);

    let mut runtime = Runtime::default();
    runtime.host_mut().register(HostFunction::new(
        "host.fail_init",
        vec![],
        "unit",
        move |_| {
            let mut count = counter.lock().expect("counter lock should succeed");
            *count += 1;
            Err(kagari_runtime::host::HostError::new("boom"))
        },
    ));

    let (_, loaded) = load_bytecode_module(
        "module_init_failed.kgr",
        BytecodeModule {
            module_init: Some(FunctionRef::new(0)),
            module_slots: vec![],
            functions: vec![BytecodeFunction {
                id: FunctionRef::new(0),
                name: "__module_init__".to_owned(),
                parameter_count: 0,
                register_count: 0,
                local_count: 0,
                instructions: vec![
                    BytecodeInstruction::Call {
                        dst: None,
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                            "host.fail_init".to_owned(),
                        )),
                        args: vec![],
                    },
                    BytecodeInstruction::Return(None),
                ],
            }],
        },
    );

    let mut vm = Vm::new(runtime);
    let first = vm
        .execute_module(&loaded)
        .expect_err("module init should fail");
    let second = vm
        .execute_module(&loaded)
        .expect_err("failed module should stay failed");

    assert!(matches!(first, VmError::HostError(ref err) if err.message() == "boom"));
    assert!(matches!(second, VmError::HostError(ref err) if err.message() == "boom"));
    assert_eq!(*init_count.lock().expect("counter lock should succeed"), 1);
}
