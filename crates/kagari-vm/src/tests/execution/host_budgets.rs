use super::*;

#[test]
fn host_runtime_helpers_enforce_capability_requirements_before_invocation() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_host = Arc::clone(&calls);
    let mut metadata = HostFunctionDeclaration::new(
        "host.secure",
        vec![],
        kagari_common::host_interface::HostValueType::I32,
    );
    metadata.capability_requirements = CapabilitySet {
        fs_read: true,
        ..CapabilitySet::default()
    };

    let mut runtime = host_call_runtime();
    runtime
        .register_host_function(HostFunction::new(metadata.clone(), move |_, _| {
            *calls_for_host
                .lock()
                .expect("host call counter should lock") += 1;
            Ok(Value::I32(1))
        }))
        .expect("host function should register");
    let loaded = runtime
        .load_program(
            "host_capability.kbc",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    verified_module(vec![test_function(
                        0,
                        "main",
                        vec![
                            BytecodeInstruction::Call {
                                dst: Some(Register::new(0)),
                                callee: CallTarget::Native(NativeCall::Host(
                                    kagari_bytecode::HostImportId::new(0),
                                )),
                                args: vec![],
                            },
                            BytecodeInstruction::Return(Some(Register::new(0))),
                        ],
                        ValueType::I32,
                        vec![ValueType::I32],
                    )]),
                    vec![metadata.clone()],
                )],
            },
        )
        .expect("module should load");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("host helper should be denied");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("fs_read")
    ));
    assert_eq!(*calls.lock().expect("host call counter should lock"), 0);
}

#[test]
fn host_runtime_helpers_charge_resource_cost_before_invocation() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_host = Arc::clone(&calls);
    let mut metadata = HostFunctionDeclaration::new(
        "host.costly",
        vec![],
        kagari_common::host_interface::HostValueType::I32,
    );
    metadata.resource_cost_hint = Some(2);

    let mut runtime = Runtime::new(RuntimeConfig {
        security: kagari_runtime::SecurityContext {
            profile: kagari_runtime::LanguageProfile {
                allow_host_calls: true,
                ..kagari_runtime::LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..CapabilitySet::default()
            },
        },
        resources: ResourcePolicy {
            max_instruction_steps: Some(2),
            ..ResourcePolicy::default()
        },
        host_exposure: kagari_runtime::HostExposurePolicy {
            allowed_host_functions: vec!["host.costly".to_owned()],
            ..kagari_runtime::HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    runtime
        .register_host_function(HostFunction::new(metadata.clone(), move |_, _| {
            *calls_for_host
                .lock()
                .expect("host call counter should lock") += 1;
            Ok(Value::I32(1))
        }))
        .expect("host function should register");
    let loaded = runtime
        .load_program(
            "host_cost.kbc",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    verified_module(vec![test_function(
                        0,
                        "main",
                        vec![
                            BytecodeInstruction::Call {
                                dst: Some(Register::new(0)),
                                callee: CallTarget::Native(NativeCall::Host(
                                    kagari_bytecode::HostImportId::new(0),
                                )),
                                args: vec![],
                            },
                            BytecodeInstruction::Return(Some(Register::new(0))),
                        ],
                        ValueType::I32,
                        vec![ValueType::I32],
                    )]),
                    vec![metadata.clone()],
                )],
            },
        )
        .expect("module should load");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("host helper should hit cost limit");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
    ));
    assert_eq!(*calls.lock().expect("host call counter should lock"), 0);
}

#[test]
fn host_runtime_helpers_enforce_host_call_resource_limit_before_invocation() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_host = Arc::clone(&calls);

    let mut runtime = Runtime::new(RuntimeConfig {
        security: kagari_runtime::SecurityContext {
            profile: kagari_runtime::LanguageProfile {
                allow_host_calls: true,
                ..kagari_runtime::LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..CapabilitySet::default()
            },
        },
        resources: ResourcePolicy {
            max_host_calls: Some(0),
            ..ResourcePolicy::default()
        },
        host_exposure: kagari_runtime::HostExposurePolicy {
            allowed_host_functions: vec!["host.limited".to_owned()],
            ..kagari_runtime::HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.limited",
                vec![],
                kagari_common::host_interface::HostValueType::I32,
            ),
            move |_, _| {
                *calls_for_host
                    .lock()
                    .expect("host call counter should lock") += 1;
                Ok(Value::I32(1))
            },
        ))
        .expect("host function should register");
    let loaded = runtime
        .load_program(
            "host_call_limit.kbc",
            kagari_bytecode::BytecodeProgram {
                root: kagari_bytecode::ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    verified_module(vec![test_function(
                        0,
                        "main",
                        vec![
                            BytecodeInstruction::Call {
                                dst: Some(Register::new(0)),
                                callee: CallTarget::Native(NativeCall::Host(
                                    kagari_bytecode::HostImportId::new(0),
                                )),
                                args: vec![],
                            },
                            BytecodeInstruction::Return(Some(Register::new(0))),
                        ],
                        ValueType::I32,
                        vec![ValueType::I32],
                    )]),
                    vec![kagari_common::host_interface::HostFunctionDeclaration::new(
                        "host.limited",
                        vec![],
                        kagari_common::host_interface::HostValueType::I32,
                    )],
                )],
            },
        )
        .expect("module should load");

    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("host helper should hit host call limit");

    assert!(matches!(
        error,
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
                && error.message().contains("host calls")
    ));
    assert_eq!(*calls.lock().expect("host call counter should lock"), 0);
    assert_eq!(vm.runtime().resources().counters().host_calls, 0);
}
