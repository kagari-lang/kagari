use kagari_ir::{
    bytecode::{
        BytecodeInstruction, CallTarget, ConstantOperand, Register, RuntimeHelper, StructFieldInit,
    },
    module::ValueType,
};
use kagari_runtime::{
    CapabilitySet, DebugVisibilityPolicy, HostExposurePolicy, LanguageProfile, ResourcePolicy,
    Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext, host::HostFunction, value::Value,
};

use crate::{
    DebugSession, Vm, VmError,
    tests::common::{compile_test_bytecode, test_function_module},
};

fn expect_capability_denied(error: VmError, capability: &str) {
    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains(capability)
    ));
}

fn expect_resource_limit(error: VmError, limit: &str) {
    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
                && error.message().contains(limit)
    ));
}

#[test]
fn security_denied_host_reflection_and_debugger_operations_are_classified() {
    let mut host_runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..CapabilitySet::default()
            },
        },
        ..RuntimeConfig::default()
    });
    let host_module = host_runtime
        .load_module(
            "security_host_denied.kbc",
            test_function_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(0)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                            "host.hidden".to_owned(),
                        )),
                        args: vec![],
                    },
                    BytecodeInstruction::Return(Some(Register::new(0))),
                ],
                ValueType::I32,
                vec![ValueType::I32],
            ),
        )
        .expect("module should load");
    let mut host_vm = Vm::new(host_runtime);
    expect_capability_denied(
        host_vm
            .execute(&host_module, "main")
            .expect_err("unexposed host helper should be denied"),
        "host function `host.hidden`",
    );

    let mut reflection_runtime = Runtime::default();
    let reflection_module = reflection_runtime
        .load_module(
            "security_reflection_denied.kbc",
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
        )
        .expect("module should load");
    let mut reflection_vm = Vm::new(reflection_runtime);
    expect_capability_denied(
        reflection_vm
            .execute(&reflection_module, "main")
            .expect_err("reflection metadata helper should be denied"),
        "reflection_metadata",
    );

    let debugger_runtime = Runtime::default();
    expect_capability_denied(
        DebugSession::new(&debugger_runtime).expect_err("debugger attach should be denied"),
        "debug_attach",
    );
}

#[test]
fn security_reflection_and_debugger_gates_remain_separate() {
    let mut metadata_only = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                reflection_metadata: true,
                reflection_read: true,
                ..CapabilitySet::default()
            },
        },
        ..RuntimeConfig::default()
    });
    let reflection_module = metadata_only
        .load_module(
            "security_reflection_write_denied.kbc",
            test_function_module(
                "main",
                vec![
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
            ),
        )
        .expect("module should load");
    let mut reflection_vm = Vm::new(metadata_only);
    expect_capability_denied(
        reflection_vm
            .execute(&reflection_module, "main")
            .expect_err("reflection write should require a separate gate"),
        "reflection_write",
    );

    let debug_runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                debug_attach: true,
                debug_breakpoints: true,
                ..CapabilitySet::default()
            },
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec!["debug.kgr".to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let mut session = DebugSession::new(&debug_runtime).expect("attach should be allowed");
    session
        .add_breakpoint(crate::SourceBreakpoint::at_source_offset("debug.kgr", 0))
        .expect("breakpoints should be allowed");
    expect_capability_denied(
        session
            .step_into()
            .expect_err("pause control should require a separate gate"),
        "debug_pause",
    );
}

#[test]
fn security_resource_limit_failures_are_classified_in_interpreter() {
    let mut instruction_limited = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_instruction_steps: Some(1),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let instruction_module = instruction_limited
        .load_module(
            "security_instruction_limit.kgr",
            compile_test_bytecode("fn main() -> i32 { 1 + 2 }"),
        )
        .expect("module should load");
    let mut instruction_vm = Vm::new(instruction_limited);
    expect_resource_limit(
        instruction_vm
            .execute(&instruction_module, "main")
            .expect_err("instruction limit should be enforced"),
        "instruction steps",
    );

    let mut allocation_limited = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_allocation_units: Some(1),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let allocation_module = allocation_limited
        .load_module(
            "security_allocation_limit.kgr",
            compile_test_bytecode("fn main() -> i32 { val values = [1, 2]; 0 }"),
        )
        .expect("module should load");
    let mut allocation_vm = Vm::new(allocation_limited);
    expect_resource_limit(
        allocation_vm
            .execute(&allocation_module, "main")
            .expect_err("allocation limit should be enforced"),
        "allocation units",
    );

    let mut host_call_limited = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec!["host.limited".to_owned()],
            ..HostExposurePolicy::default()
        },
        resources: ResourcePolicy {
            max_host_calls: Some(0),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    host_call_limited
        .register_host_function(HostFunction::new("host.limited", vec![], "i32", |_| {
            Ok(Value::I32(1))
        }))
        .expect("host function should register");
    let host_module = host_call_limited
        .load_module(
            "security_host_call_limit.kbc",
            test_function_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(0)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                            "host.limited".to_owned(),
                        )),
                        args: vec![],
                    },
                    BytecodeInstruction::Return(Some(Register::new(0))),
                ],
                ValueType::I32,
                vec![ValueType::I32],
            ),
        )
        .expect("module should load");
    let mut host_vm = Vm::new(host_call_limited);
    expect_resource_limit(
        host_vm
            .execute(&host_module, "main")
            .expect_err("host call limit should be enforced"),
        "host calls",
    );

    let mut reflection_limited = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_reflection: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                reflection_metadata: true,
                reflection_read: true,
                ..CapabilitySet::default()
            },
        },
        resources: ResourcePolicy {
            max_reflection_operations: Some(1),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let reflection_module = reflection_limited
        .load_module(
            "security_reflection_limit.kbc",
            test_function_module(
                "main",
                vec![
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
            ),
        )
        .expect("module should load");
    let mut reflection_vm = Vm::new(reflection_limited);
    expect_resource_limit(
        reflection_vm
            .execute(&reflection_module, "main")
            .expect_err("reflection operation limit should be enforced"),
        "reflection operations",
    );
}
