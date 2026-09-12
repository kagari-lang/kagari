use kagari_ir::bytecode::{
    BinaryOp, BytecodeFunction, BytecodeInstruction, BytecodeModule, CallTarget, ConstantOperand,
    FunctionRef, PathId, PathRecord, Register, RuntimeHelper, StandardIntrinsic, StructId,
};
use kagari_ir::module::ValueType;
use kagari_runtime::host::PreparedHostPathWrite;
use std::sync::{Arc, Mutex};

use kagari_runtime::{
    AbiFingerprint, CapabilitySet, FieldMetadataId, HostExposurePolicy, HostObjectId,
    HostPathAdapter, HostPathDescriptorRegistration, HostPathSegment, HostReflectionPolicy,
    HostSchemaEpoch, HostTypeOwnership, HostTypeRegistration, LanguageProfile, PathAccess,
    ResourcePolicy, Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext, TypeKind,
    TypeRegistration,
    host::{HostError, HostFunction},
    value::Value,
};

use crate::Vm;
use crate::tests::common::{
    compile_test_bytecode, load_bytecode_module, load_bytecode_module_with_runtime,
    load_test_module, test_function_module,
};

fn host_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                allow_path_mutation: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                path_mutation: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec![
                "host.player".to_owned(),
                "host.add_i32".to_owned(),
                "host.log".to_owned(),
            ],
            allowed_host_types: vec!["game.Player".to_owned()],
            allow_host_path_reads: true,
            allow_host_path_mutation: true,
            ..HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    })
}

fn reflection_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                reflection_metadata: true,
                reflection_read: true,
                reflection_write: true,
                ..CapabilitySet::default()
            },
        },
        ..RuntimeConfig::default()
    })
}

fn load_reflection_bytecode_module(
    name: &str,
    bytecode: BytecodeModule,
) -> (Runtime, kagari_runtime::LoadedModule) {
    load_bytecode_module_with_runtime(reflection_runtime(), name, bytecode)
}

fn load_reflection_test_module(source_text: &str) -> (Runtime, kagari_runtime::LoadedModule) {
    load_reflection_bytecode_module("test.kgr", compile_test_bytecode(source_text))
}

fn register_vm_host_path_runtime(access: PathAccess) -> (Runtime, Arc<Mutex<i32>>) {
    register_vm_host_path_runtime_with_capabilities(access, CapabilitySet::default())
}

fn register_vm_host_path_runtime_with_capabilities(
    access: PathAccess,
    capability_requirements: CapabilitySet,
) -> (Runtime, Arc<Mutex<i32>>) {
    let mut runtime = host_runtime();
    let i32_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(1),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap();
    let mut host_type = HostTypeRegistration::new("game.Player", "game.Player");
    host_type.ownership = HostTypeOwnership::HostRoot;
    host_type.path_access = PathAccess::ReadWrite;
    host_type.reflection = HostReflectionPolicy::Hidden;
    host_type.abi_fingerprint = AbiFingerprint(2);
    let player_id = runtime.register_host_type(host_type).unwrap();
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.player",
                vec![],
                kagari_common::host_interface::HostValueType::opaque("game.Player"),
            ),
            move |_| Ok(Value::HostRoot(root)),
        ))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegment::Field {
                name: "hp".to_owned(),
                field_id: FieldMetadataId::new(0),
                owner_type: player_id,
                result_type: i32_id,
                access,
                abi_fingerprint: AbiFingerprint(3),
            }],
            access,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(4),
            capability_requirements,
        })
        .unwrap();
    assert_eq!(descriptor_id.index(), 0);

    let hp = Arc::new(Mutex::new(10));
    let read_hp = Arc::clone(&hp);
    let write_hp = Arc::clone(&hp);
    runtime
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new()
                .with_read(move |_| Ok(Value::I32(*read_hp.lock().unwrap())))
                .with_prepare_write(move |_, record| {
                    let Value::I32(value) = record.new_value else {
                        return Err(HostError::new("hp expects i32"));
                    };
                    let write_hp = write_hp.clone();
                    Ok(PreparedHostPathWrite::new(move || {
                        *write_hp.lock().unwrap() = value;
                    }))
                }),
        )
        .unwrap();

    (runtime, hp)
}

fn path_module(
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
) -> BytecodeModule {
    let instructions_constants = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::LoadConst { constant, .. } => Some(constant.clone()),
            _ => None,
        })
        .collect();
    let metadata = kagari_ir::bytecode::FunctionMetadata {
        return_type,
        registers: vec![
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::HeapObject,
        ],
        ..Default::default()
    };
    BytecodeModule {
        host_interface: kagari_common::host_interface::HostInterface {
            functions: vec![kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.player",
                vec![],
                kagari_common::host_interface::HostValueType::opaque("game.Player"),
            )],
        },
        module_init: None,
        module_slots: vec![],
        constants: instructions_constants,
        types: vec![ValueType::Unit, ValueType::HeapObject, ValueType::I32],
        paths: vec![PathRecord {
            id: PathId::new(0),
            root_ty: ValueType::HeapObject,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "game.Player.hp".to_owned(),
        }],
        function_table: vec![kagari_ir::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: name.to_owned(),
            params: metadata.params.clone(),
            return_type: metadata.return_type,
            effects: metadata.effects,
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: name.to_owned(),
            parameter_count: 0,
            register_count: metadata.registers.len() as u16,
            local_count: 0,
            metadata,
            instructions,
        }],
        ..Default::default()
    }
}

#[test]
fn executes_runtime_host_helper_call() {
    let mut runtime = host_runtime();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.add_i32",
                vec![
                    kagari_common::host_interface::HostParameter {
                        name: "lhs".into(),
                        ty: kagari_common::host_interface::HostValueType::I32,
                        passing: kagari_common::host_interface::HostPassingStyle::Owned,
                    },
                    kagari_common::host_interface::HostParameter {
                        name: "rhs".into(),
                        ty: kagari_common::host_interface::HostValueType::I32,
                        passing: kagari_common::host_interface::HostPassingStyle::Owned,
                    },
                ],
                kagari_common::host_interface::HostValueType::I32,
            ),
            |args| match args {
                [Value::I32(lhs), Value::I32(rhs)] => Ok(Value::I32(lhs + rhs)),
                _ => Err(HostError::new("host.add_i32 expects two i32 arguments")),
            },
        ))
        .expect("host function should register");

    let loaded = runtime
        .load_program(
            "helper.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![crate::tests::common::with_host_imports(
                    test_function_module(
                        "main",
                        vec![
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
                                callee: CallTarget::HostFunction(
                                    kagari_ir::bytecode::HostImportId::new(0),
                                ),
                                args: vec![Register::new(0), Register::new(1)],
                            },
                            BytecodeInstruction::Return(Some(Register::new(2))),
                        ],
                        ValueType::I32,
                        vec![ValueType::I32, ValueType::I32, ValueType::I32],
                    ),
                    vec![kagari_common::host_interface::HostFunctionDeclaration::new(
                        "host.add_i32",
                        vec![
                            kagari_common::host_interface::HostParameter {
                                name: "lhs".into(),
                                ty: kagari_common::host_interface::HostValueType::I32,
                                passing: kagari_common::host_interface::HostPassingStyle::Owned,
                            },
                            kagari_common::host_interface::HostParameter {
                                name: "rhs".into(),
                                ty: kagari_common::host_interface::HostValueType::I32,
                                passing: kagari_common::host_interface::HostPassingStyle::Owned,
                            },
                        ],
                        kagari_common::host_interface::HostValueType::I32,
                    )],
                )],
            },
        )
        .expect("helper module should load");

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(42));
}

#[test]
fn path_commit_faults_release_frames_and_prevent_further_interpreter_or_jit_execution() {
    use kagari_ir::bytecode::{BytecodeProgram, KbcArtifact, ModuleRef};
    for initializing in [false, true] {
        for encoded in [false, true] {
            for jit in [false, true] {
                let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
                let mut security = runtime.security();
                security.profile.allow_jit = true;
                security.capabilities.jit = true;
                runtime.set_security_context(security);
                let write_hp = hp.clone();
                runtime
                    .register_host_path_adapter(
                        kagari_runtime::HostPathDescriptorId::new(0),
                        HostPathAdapter::new()
                            .with_read(|_| Ok(Value::I32(10)))
                            .with_prepare_write(move |_, record| {
                                let Value::I32(value) = record.new_value else {
                                    return Err(HostError::new("expected i32"));
                                };
                                let hp = write_hp.clone();
                                Ok(PreparedHostPathWrite::new(move || {
                                    *hp.lock().unwrap() = value;
                                    panic!("host commit violated its contract");
                                }))
                            }),
                    )
                    .unwrap();
                let scalar = runtime
                    .load_program(
                        "scalar.kgr",
                        BytecodeProgram {
                            root: ModuleRef::new(0),
                            modules: vec![compile_test_bytecode("fn main() -> i32 { 42 }")],
                        },
                    )
                    .unwrap();
                let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
                let native = kagari_runtime::CodegenBackend::compile_function(
                    &mut backend,
                    kagari_runtime::BackendFunctionInput::new(&scalar, FunctionRef::new(0))
                        .unwrap(),
                )
                .unwrap();
                let mut program = BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![path_module(
                        "main",
                        vec![
                            BytecodeInstruction::Call {
                                dst: Some(Register::new(0)),
                                callee: CallTarget::HostFunction(
                                    kagari_ir::bytecode::HostImportId::new(0),
                                ),
                                args: vec![],
                            },
                            BytecodeInstruction::LoadConst {
                                dst: Register::new(1),
                                constant: ConstantOperand::I32(20),
                            },
                            BytecodeInstruction::SetPath {
                                root_or_view: Register::new(0),
                                path: PathId::new(0),
                                dynamic_args: vec![],
                                value: Register::new(1),
                            },
                            BytecodeInstruction::Return(Some(Register::new(1))),
                        ],
                        ValueType::I32,
                    )],
                };
                if initializing {
                    program.modules[0].module_init = Some(FunctionRef::new(0));
                }
                let program = if encoded {
                    let artifact = KbcArtifact::from_program(program, Default::default());
                    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
                    decoded.validate_for_loader(&Default::default()).unwrap();
                    decoded.program
                } else {
                    program
                };
                let loaded = runtime.load_program("fault.kgr", program).unwrap();
                let mut vm = Vm::new(runtime);
                let error = if jit {
                    vm.execute_with_backend(&loaded, "main", &mut backend)
                        .unwrap_err()
                } else {
                    vm.execute(&loaded, "main").unwrap_err()
                };
                assert!(
                    matches!(error, crate::VmError::RuntimeError(ref error) if error.kind() == RuntimeErrorKind::EngineFault)
                );
                assert!(vm.runtime().is_quarantined());
                assert_eq!(
                    vm.runtime()
                        .modules()
                        .instance_snapshot(loaded.key())
                        .unwrap()
                        .state,
                    if initializing {
                        kagari_runtime::ModuleInitializationState::Failed
                    } else {
                        kagari_runtime::ModuleInitializationState::Initialized
                    }
                );
                assert_eq!(
                    vm.runtime()
                        .modules()
                        .retention_counts(loaded.key())
                        .active_calls,
                    0
                );
                assert_eq!(*hp.lock().unwrap(), 20); // Internal faults do not promise business rollback.
                assert_eq!(vm.runtime().gc().active_roots(), 0);
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                let before = vm.runtime().resources().counters();
                for error in [
                    vm.execute(&scalar, "main").unwrap_err(),
                    vm.execute_with_backend(&scalar, "main", &mut backend)
                        .unwrap_err(),
                ] {
                    assert!(
                        matches!(error, crate::VmError::RuntimeError(ref error) if error.kind() == RuntimeErrorKind::EngineFault)
                    );
                }
                assert!(
                    matches!(backend.invoke_compiled_scalar(&native, vm.runtime()),
                Err(kagari_runtime::BackendInvocationError::RuntimeFailure(ref error)) if error.kind() == RuntimeErrorKind::EngineFault)
                );
                assert_eq!(
                    unsafe { kagari_runtime::jit_abi::jit_consume_instruction_step(vm.runtime()) },
                    kagari_runtime::jit_abi::JIT_STATUS_ENGINE_FAULT
                );
                assert_eq!(vm.runtime().resources().counters(), before);
            }
        }
    }
}

#[test]
fn executes_typed_path_read_set_modify_and_view_instructions() {
    let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
    let loaded = runtime
        .load_program(
            "paths.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![path_module(
                    "main",
                    vec![
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(0)),
                            callee: CallTarget::HostFunction(
                                kagari_ir::bytecode::HostImportId::new(0),
                            ),
                            args: vec![],
                        },
                        BytecodeInstruction::ReadPath {
                            dst: Register::new(1),
                            root_or_view: Register::new(0),
                            path: PathId::new(0),
                            dynamic_args: vec![],
                        },
                        BytecodeInstruction::LoadConst {
                            dst: Register::new(2),
                            constant: ConstantOperand::I32(5),
                        },
                        BytecodeInstruction::SetPath {
                            root_or_view: Register::new(0),
                            path: PathId::new(0),
                            dynamic_args: vec![],
                            value: Register::new(2),
                        },
                        BytecodeInstruction::LoadConst {
                            dst: Register::new(3),
                            constant: ConstantOperand::I32(2),
                        },
                        BytecodeInstruction::ModifyPath {
                            dst: Some(Register::new(4)),
                            root_or_view: Register::new(0),
                            path: PathId::new(0),
                            dynamic_args: vec![],
                            op: BinaryOp::Add,
                            value: Register::new(3),
                        },
                        BytecodeInstruction::MakePathView {
                            dst: Register::new(5),
                            root_or_view: Register::new(0),
                            path: PathId::new(0),
                            dynamic_args: vec![],
                        },
                        BytecodeInstruction::Return(Some(Register::new(4))),
                    ],
                    ValueType::I32,
                )],
            },
        )
        .unwrap();

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").unwrap();

    assert_eq!(report.return_value, Value::I32(7));
    assert_eq!(*hp.lock().unwrap(), 7);
    assert_eq!(vm.runtime().host_dirty_paths().len(), 2);
}

#[test]
fn typed_path_instruction_failures_are_runtime_typed_path_errors() {
    let (mut runtime, _) = register_vm_host_path_runtime(PathAccess::ReadOnly);
    let loaded = runtime
        .load_program(
            "readonly_path.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![path_module(
                    "main",
                    vec![
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(0)),
                            callee: CallTarget::HostFunction(
                                kagari_ir::bytecode::HostImportId::new(0),
                            ),
                            args: vec![],
                        },
                        BytecodeInstruction::LoadConst {
                            dst: Register::new(1),
                            constant: ConstantOperand::I32(2),
                        },
                        BytecodeInstruction::SetPath {
                            root_or_view: Register::new(0),
                            path: PathId::new(0),
                            dynamic_args: vec![],
                            value: Register::new(1),
                        },
                        BytecodeInstruction::Return(None),
                    ],
                    ValueType::Unit,
                )],
            },
        )
        .unwrap();

    let mut vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();

    assert!(matches!(
        error,
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::TypedPathValidation
    ));
}

#[test]
fn typed_path_helpers_enforce_runtime_capability_boundary() {
    let (mut runtime, _) = register_vm_host_path_runtime_with_capabilities(
        PathAccess::ReadWrite,
        CapabilitySet {
            fs_read: true,
            ..CapabilitySet::default()
        },
    );
    let loaded = runtime
        .load_program(
            "path_capability.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![path_module(
                    "main",
                    vec![
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(0)),
                            callee: CallTarget::HostFunction(
                                kagari_ir::bytecode::HostImportId::new(0),
                            ),
                            args: vec![],
                        },
                        BytecodeInstruction::ReadPath {
                            dst: Register::new(1),
                            root_or_view: Register::new(0),
                            path: PathId::new(0),
                            dynamic_args: vec![],
                        },
                        BytecodeInstruction::Return(Some(Register::new(1))),
                    ],
                    ValueType::I32,
                )],
            },
        )
        .unwrap();

    let mut vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();

    assert!(matches!(
        error,
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("fs_read")
    ));
}

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
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("reflection_metadata")
    ));
}

#[test]
fn reflection_metadata_and_read_gates_are_separate() {
    let mut metadata_only = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_reflection: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                reflection_metadata: true,
                ..CapabilitySet::default()
            },
        },
        ..RuntimeConfig::default()
    });
    let loaded = metadata_only
        .load_program(
            "reflect_read_denied.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
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
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("reflection_read")
    ));
}

#[test]
fn reflection_read_and_write_gates_are_separate() {
    let mut read_only = Runtime::new(RuntimeConfig {
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
    let loaded = read_only
        .load_program(
            "reflect_write_denied.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
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
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("reflection_write")
    ));
}

#[test]
fn reflection_helpers_enforce_reflection_operation_resource_limit() {
    let mut runtime = Runtime::new(RuntimeConfig {
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
    let loaded = runtime
        .load_program(
            "reflect_operation_limit.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
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
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
                && error.message().contains("reflection operations")
    ));
    assert_eq!(vm.runtime().resources().counters().reflection_operations, 1);
}

#[test]
fn executes_runtime_reflect_get_and_set_field_helpers() {
    let (runtime, loaded) = load_reflection_bytecode_module(
        "reflect_field.kbc",
        super::common::point_function_module(
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
fn executes_source_lowered_print_builtin() {
    let messages = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&messages);

    let mut runtime = host_runtime();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::standard_log(),
            move |args| {
                let Some(Value::Str(message)) = args.first() else {
                    return Err(HostError::new("host.log expects one string argument"));
                };
                sink.lock()
                    .expect("message sink should lock")
                    .push(message.clone());
                Ok(Value::Unit)
            },
        ))
        .expect("host function should register");
    let bytecode = compile_test_bytecode(r#"fn main() { print("hello"); }"#);
    let loaded = runtime
        .load_program(
            "print.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect("print module should load");

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::Unit);
    assert_eq!(
        *messages.lock().expect("message sink should lock"),
        vec!["hello".to_string()]
    );
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
fn main() -> [i32] {
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
fn executes_source_lowered_standard_intrinsics() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> (usize, usize, i32, bool) {
    val values = [1, 2];
    values.push(3);
    val popped = values.pop();
    (values.len(), "kagari".len_chars(), std::math::clamp(4, 1, 3), popped.is_some())
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![
            Value::I64(2),
            Value::I64(6),
            Value::I32(3),
            Value::Bool(true),
        ])
    );
}

#[test]
fn executes_source_standard_collection_string_math_and_debug_modules() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> (usize, bool, usize, usize, usize, usize, usize, usize, bool, bool, bool, i32) {
    val map: Map<String, i32> = std::map::new();
    map.insert("a", 1);
    map.insert("b", 2);
    val keys = map.keys();
    val values = map.values();
    val entries = map.entries();

    val set: Set<String> = std::set::new();
    set.insert("x");
    set.insert("y");
    val set_items = set.to_array();
    val union = set.union(set);

    std::debug::assert(map.contains_key("a"), "map key must exist");
    (
        map.len(),
        map.contains_key("a"),
        keys.len(),
        values.len(),
        entries.len(),
        set.len(),
        set_items.len(),
        union.len(),
        std::string::contains("kagari", "gar"),
        std::string::starts_with("kagari", "ka"),
        std::string::ends_with("kagari", "ri"),
        std::math::max(1, 2)
    )
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![
            Value::I64(2),
            Value::Bool(true),
            Value::I64(2),
            Value::I64(2),
            Value::I64(2),
            Value::I64(2),
            Value::I64(2),
            Value::I64(2),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::I32(2),
        ])
    );
}

#[test]
fn executes_bytecode_standard_collection_intrinsics() {
    let (runtime, loaded) = load_bytecode_module(
        "standard_collections.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::Str("k".to_owned()),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(1)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapNew),
                    args: vec![],
                },
                BytecodeInstruction::LoadConst {
                    dst: Register::new(2),
                    constant: ConstantOperand::I32(7),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(3)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapInsert),
                    args: vec![Register::new(1), Register::new(0), Register::new(2)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(4)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapLen),
                    args: vec![Register::new(1)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(5)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapContainsKey),
                    args: vec![Register::new(1), Register::new(0)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(6)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapKeys),
                    args: vec![Register::new(1)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(7)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayLen),
                    args: vec![Register::new(6)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(8)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::SetNew),
                    args: vec![],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(9)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::SetInsert),
                    args: vec![Register::new(8), Register::new(0)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(10)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::SetContains),
                    args: vec![Register::new(8), Register::new(0)],
                },
                BytecodeInstruction::MakeTuple {
                    dst: Register::new(11),
                    elements: vec![
                        Register::new(4),
                        Register::new(5),
                        Register::new(7),
                        Register::new(10),
                    ],
                },
                BytecodeInstruction::Return(Some(Register::new(11))),
            ],
            ValueType::HeapObject,
            vec![
                ValueType::Str,
                ValueType::HeapObject,
                ValueType::I32,
                ValueType::HeapObject,
                ValueType::I64,
                ValueType::Bool,
                ValueType::HeapObject,
                ValueType::I64,
                ValueType::HeapObject,
                ValueType::HeapObject,
                ValueType::Bool,
                ValueType::HeapObject,
            ],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![
            Value::I64(1),
            Value::Bool(true),
            Value::I64(1),
            Value::Bool(true),
        ])
    );
}

#[test]
fn standard_intrinsics_reject_invalid_hash_keys_before_publication() {
    let bytecode = test_function_module(
        "main",
        vec![
            BytecodeInstruction::Call {
                dst: Some(Register::new(0)),
                callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapNew),
                args: vec![],
            },
            BytecodeInstruction::LoadConst {
                dst: Register::new(1),
                constant: ConstantOperand::I32(1),
            },
            BytecodeInstruction::MakeArray {
                dst: Register::new(2),
                elements: vec![Register::new(1)],
            },
            BytecodeInstruction::Call {
                dst: Some(Register::new(3)),
                callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapInsert),
                args: vec![Register::new(0), Register::new(2), Register::new(1)],
            },
            BytecodeInstruction::Return(Some(Register::new(3))),
        ],
        ValueType::HeapObject,
        vec![
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::HeapObject,
            ValueType::HeapObject,
        ],
    );
    let mut runtime = Runtime::default();

    let error = runtime
        .load_program(
            "standard_invalid_key.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect_err("aggregate map key should reject before publication");

    assert!(matches!(error.kind(), RuntimeErrorKind::ModuleValidation));
    assert!(error.message().contains("hash-key"));
}

#[test]
fn standard_intrinsic_execution_observes_resource_limits() {
    let bytecode = compile_test_bytecode(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3);
    values.len()
}
"#,
    );
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_instruction_steps: Some(1),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program(
            "standard_resource_limit.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect("module should load");
    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("standard intrinsic program should hit resource limit");

    assert!(matches!(
        error,
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
    ));
}

#[test]
fn standard_collection_reflection_metadata_reports_runtime_categories() {
    let (runtime, loaded) = load_reflection_bytecode_module(
        "standard_reflection.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::Call {
                    dst: Some(Register::new(0)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapNew),
                    args: vec![],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(1)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::SetNew),
                    args: vec![],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(2)),
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
                    args: vec![Register::new(0)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(3)),
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
                    args: vec![Register::new(1)],
                },
                BytecodeInstruction::MakeTuple {
                    dst: Register::new(4),
                    elements: vec![Register::new(2), Register::new(3)],
                },
                BytecodeInstruction::Return(Some(Register::new(4))),
            ],
            ValueType::HeapObject,
            vec![
                ValueType::HeapObject,
                ValueType::HeapObject,
                ValueType::Str,
                ValueType::Str,
                ValueType::HeapObject,
            ],
        ),
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

#[test]
fn executes_source_lowered_string_len_chars_method() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> usize {
    "kagari".len_chars()
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I64(6));
}

#[test]
fn standard_array_mutation_updates_shared_array_handles() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> usize {
    val values = [1, 2];
    val alias = values;
    values.push(3);
    alias.len()
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I64(3));
}

#[test]
fn struct_field_updates_mutate_shared_struct_handle_in_place() {
    let (runtime, loaded) = load_reflection_test_module(
        r#"
struct Point { var x: i32 }

fn main() -> i32 {
    val point = Point { x: 1 };
    val alias = point;
    set_field(point, "x", 9);
    alias.x
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(9));
}
