use crate::{
    debug::{DebugSession, SourceBreakpoint},
    tests::common::{
        compile_native_test_bytecode, logging_module, module_program, standard_runtime,
        test_function_module,
    },
    vm::Vm,
    vm::fixtures::FixtureExecution,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::{
    BytecodeInstruction, CallTarget, ConstantId, ConstantOperand, Register, RuntimeHelper, StructId,
};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    RuntimeConfig,
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, types::Type,
    },
    value::Value,
    value_semantics::script_equal,
};

#[test]
fn installed_host_reflection_and_debugger_operations_are_available() {
    let mut host_runtime = standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    logging_module(|_, _| Ok(Value::Unit))
        .install(&mut host_runtime)
        .unwrap();
    let mut builder = ModuleBuilder::new("fixtures::scalar", &Default::default()).unwrap();
    let hidden = builder
        .define_function(FunctionDecl::new("hidden").returns(Type::i32()))
        .unwrap();
    builder
        .bind(hidden, |_: &mut CallContext<'_>| -> NativeResult<i32> {
            Ok(42)
        })
        .unwrap();
    let provider = builder.finish().unwrap();
    provider.install(&mut host_runtime).unwrap();
    let host_module = host_runtime
        .load_program(
            "installed-host.kbc",
            compile_native_test_bytecode(
                "use fixtures::scalar::hidden; fn main() -> i32 { hidden() }",
                &[&provider],
            ),
        )
        .expect("module should load");
    let host_vm = Vm::new(host_runtime);
    assert_eq!(
        host_vm
            .execute(&host_module, ProgramEntry::Main)
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
