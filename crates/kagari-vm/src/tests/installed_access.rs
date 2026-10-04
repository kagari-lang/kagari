use crate::{
    debug::{DebugSession, SourceBreakpoint},
    tests::common::test_function_module,
    vm::Vm,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{
        BytecodeInstruction, CallTarget, ConstantOperand, NativeImportId, Register, RuntimeHelper,
        StructId,
    },
    program::{BytecodeProgram, ModuleRef},
};
use kagari_runtime::{Runtime, RuntimeConfig, host::HostFunction, value::Value};
use kagari_types::host_interface::value_type::HostValueType;

#[test]
fn installed_host_reflection_and_debugger_operations_are_available() {
    let mut host_runtime = Runtime::new(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    host_runtime
        .register_host_function(HostFunction::new(
            kagari_types::host_interface::HostFunctionDeclaration::new(
                "host.hidden",
                vec![],
                HostValueType::I32,
            ),
            |_, _| Ok(Value::I32(42)),
        ))
        .unwrap();
    let host_module = host_runtime
        .load_program(
            "security_host_denied.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    test_function_module(
                        "main",
                        vec![
                            BytecodeInstruction::Call {
                                dst: Some(Register::new(0)),
                                callee: CallTarget::Native(NativeImportId::new(0)),
                                args: vec![],
                            },
                            BytecodeInstruction::Return(Some(Register::new(0))),
                        ],
                        ValueType::I32,
                        vec![ValueType::I32],
                    ),
                    vec![kagari_types::host_interface::HostFunctionDeclaration::new(
                        "host.hidden",
                        vec![],
                        HostValueType::I32,
                    )],
                )],
            },
        )
        .expect("module should load");
    let mut host_vm = Vm::new(host_runtime);
    assert_eq!(
        host_vm.execute(&host_module, "main").unwrap().return_value,
        Value::I32(42)
    );

    let mut reflection_runtime = Runtime::default();
    let reflection_module = reflection_runtime
        .load_program(
            "security_reflection_denied.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![test_function_module(
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
                )],
            },
        )
        .expect("module should load");
    let mut reflection_vm = Vm::new(reflection_runtime);
    assert_eq!(
        reflection_vm
            .execute(&reflection_module, "main")
            .unwrap()
            .return_value,
        Value::Str("i32".into())
    );
    DebugSession::new(&Runtime::default()).unwrap();
}

#[test]
fn reflection_mutation_and_debugger_control_need_no_permission_flags() {
    let mut metadata_only = Runtime::new(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    let reflection_module = metadata_only
        .load_program(
            "security_reflection_write_denied.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![super::common::point_function_module(
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
        .expect("module should load");
    let mut reflection_vm = Vm::new(metadata_only);
    reflection_vm.execute(&reflection_module, "main").unwrap();

    let debug_runtime = Runtime::new(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    let mut session = DebugSession::new(&debug_runtime).expect("attach should be allowed");
    session
        .add_breakpoint(SourceBreakpoint::at_source_offset("debug.kgr", 0))
        .expect("breakpoints should be allowed");
    session.step_into().unwrap();
}
