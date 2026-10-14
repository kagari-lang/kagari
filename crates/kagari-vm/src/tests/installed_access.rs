use crate::{
    debug::{DebugSession, SourceBreakpoint},
    tests::common::{module_program, standard_runtime, test_function_module, with_host_imports},
    vm::Vm,
    vm::fixtures::FixtureExecution,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::{
    BytecodeInstruction, CallTarget, ConstantId, ConstantOperand, NativeImportId, Register,
    RuntimeHelper, StructId,
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    RuntimeConfig, host::HostFunction, value::Value, value_semantics::script_equal,
};
use kagari_types::host_interface::{HostFunctionDeclaration, value_type::HostValueType};

#[test]
fn installed_host_reflection_and_debugger_operations_are_available() {
    let mut host_runtime = standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    host_runtime
        .register_host_function(HostFunction::new(
            {
                let context = DefinitionContext::new().unwrap();
                let declaration = HostFunctionDeclaration::new(
                    &context,
                    "host.hidden",
                    vec![],
                    HostValueType::I32,
                )
                .unwrap();
                DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default())
                    .unwrap()
            },
            |_, _| Ok(Value::I32(42)),
        ))
        .unwrap();
    let host_module = host_runtime
        .load_program(
            "security_host_denied.kbc",
            module_program(with_host_imports(
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
                    vec![],
                ),
                vec![{
                    let context = DefinitionContext::new().unwrap();
                    let declaration = HostFunctionDeclaration::new(
                        &context,
                        "host.hidden",
                        vec![],
                        HostValueType::I32,
                    )
                    .unwrap();
                    DefinitionMetadata::checked(
                        context.snapshot(),
                        declaration,
                        &Default::default(),
                    )
                    .unwrap()
                }],
            )),
        )
        .expect("module should load");
    let host_vm = Vm::new(host_runtime);
    assert_eq!(
        host_vm
            .execute_fixture(&host_module, FunctionRef::new(0))
            .unwrap()
            .return_value
            .value(host_vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );

    let mut reflection_runtime = standard_runtime(Default::default());
    let reflection_module = reflection_runtime
        .load_program(
            "security_reflection_denied.kbc",
            module_program(test_function_module(
                "main",
                vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantId::new(0),
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
                vec![ConstantOperand::I32(7)],
            )),
        )
        .expect("module should load");
    let reflection_vm = Vm::new(reflection_runtime);
    assert!(
        script_equal(
            reflection_vm.runtime().gc(),
            &(reflection_vm
                .execute_fixture(&reflection_module, FunctionRef::new(0))
                .unwrap()
                .return_value
                .value(reflection_vm.runtime().gc())
                .expect("retained execution result")),
            &(reflection_vm
                .runtime()
                .gc()
                .alloc_string("i32".into())
                .unwrap())
        )
        .unwrap()
    );
    DebugSession::new(&standard_runtime(Default::default())).unwrap();
}

#[test]
fn reflection_mutation_and_debugger_control_need_no_permission_flags() {
    let mut metadata_only = standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    let reflection_module = metadata_only
        .load_program(
            "security_reflection_write_denied.kbc",
            module_program(super::common::point_function_module(
                "main",
                vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantId::new(0),
                    },
                    BytecodeInstruction::MakeStruct {
                        dst: Register::new(1),
                        structure: StructId::new(0),
                        arguments: vec![],
                        fields: vec![Register::new(0)],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(2),
                        constant: ConstantId::new(1),
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
                vec![ConstantOperand::I32(1), ConstantOperand::I32(2)],
            )),
        )
        .expect("module should load");
    let reflection_vm = Vm::new(metadata_only);
    reflection_vm
        .execute_fixture(&reflection_module, FunctionRef::new(0))
        .unwrap();

    let debug_runtime = standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    let mut session = DebugSession::new(&debug_runtime).expect("attach should be allowed");
    session
        .add_breakpoint(SourceBreakpoint::at_source_offset("debug.kgr", 0))
        .expect("breakpoints should be allowed");
    session.step_into().unwrap();
}
