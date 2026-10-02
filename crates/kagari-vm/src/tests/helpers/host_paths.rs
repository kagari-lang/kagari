use super::*;
use kagari_bytecode::instruction::NativeImportId;
use {crate::error::VmError, kagari_common::host_interface::value_type::HostValueType};
use {crate::reentry::reenter, kagari_runtime::host::HostPathDescriptorId};

use crate::{tests::native_fixtures, vm::native::PreparedNativeEntry};

use kagari_bytecode::program::{BytecodeProgram, ModuleRef};
use kagari_runtime::backend::{BackendInvocationError, native::NativeInvocationFailure};

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
                        ty: HostValueType::I32,
                        passing: kagari_common::host_interface::HostPassingStyle::Owned,
                    },
                    kagari_common::host_interface::HostParameter {
                        name: "rhs".into(),
                        ty: HostValueType::I32,
                        passing: kagari_common::host_interface::HostPassingStyle::Owned,
                    },
                ],
                HostValueType::I32,
            ),
            |_, args| match args {
                [Value::I32(lhs), Value::I32(rhs)] => Ok(Value::I32(lhs + rhs)),
                _ => Err(HostError::new("host.add_i32 expects two i32 arguments")),
            },
        ))
        .expect("host function should register");

    let loaded = runtime
        .load_program(
            "helper.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
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
                                callee: CallTarget::Native(NativeImportId::new(0)),
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
                                ty: HostValueType::I32,
                                passing: kagari_common::host_interface::HostPassingStyle::Owned,
                            },
                            kagari_common::host_interface::HostParameter {
                                name: "rhs".into(),
                                ty: HostValueType::I32,
                                passing: kagari_common::host_interface::HostPassingStyle::Owned,
                            },
                        ],
                        HostValueType::I32,
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
    use kagari_bytecode::artifact::KbcArtifact;
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
                    HostPathDescriptorId::new(0),
                    HostPathAdapter::new()
                        .with_read(|_, _| Ok(Value::I32(10)))
                        .with_prepare_write(move |_, _, record| {
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
                    compile_test_bytecode("fn main() -> i32 { 42 }"),
                )
                .unwrap();
            let prepared = native_fixtures::unsupported();
            let native = native_fixtures::install_i32::<42>(&runtime, &scalar, false);
            let scalar_entry = PreparedNativeEntry::Native(native.clone());
            let program = BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![path_module(
                    &runtime,
                    "main",
                    vec![
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(0)),
                            callee: CallTarget::Native(NativeImportId::new(0)),
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
            let program = if encoded {
                let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
                let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
                decoded.validate_for_loader(&Default::default()).unwrap();
                decoded.program
            } else {
                program
            };
            let loaded = runtime.load_program("fault.kgr", program).unwrap();
            let mut vm = Vm::new(runtime);
            let error = if jit {
                vm.execute_prepared(&loaded, "main", &prepared).unwrap_err()
            } else {
                vm.execute(&loaded, "main").unwrap_err()
            };
            assert!(
                matches!(error, VmError::RuntimeError(ref error) if error.kind() == RuntimeErrorKind::EngineFault)
            );
            assert!(vm.runtime().is_quarantined());
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
                vm.execute_prepared(&scalar, "main", &scalar_entry)
                    .unwrap_err(),
            ] {
                assert!(
                    matches!(error, VmError::RuntimeError(ref error) if error.kind() == RuntimeErrorKind::EngineFault)
                );
            }
            assert!(matches!(vm.runtime().invoke_native_function(&native),
                Err(NativeInvocationFailure { error: BackendInvocationError::RuntimeFailure(ref error), .. }) if error.kind() == RuntimeErrorKind::EngineFault));
            assert_eq!(
                unsafe { kagari_runtime::jit_abi::jit_consume_instruction_step(vm.runtime(), 0) },
                kagari_abi::native_call::JIT_STATUS_ENGINE_FAULT
            );
            assert_eq!(vm.runtime().resources().counters(), before);
        }
    }
}

#[test]
fn typed_path_callbacks_reenter_the_root_session_before_commit() {
    use kagari_bytecode::artifact::KbcArtifact;
    use std::{cell::RefCell, rc::Rc};
    fn assert_reentry(call: &kagari_runtime::host::HostCallContext<'_>, function: FunctionRef) {
        let root = call.runtime().execution_root().unwrap();
        let scope = call
            .runtime()
            .begin_execution(&root, call.runtime().execution_options())
            .unwrap();
        assert_eq!(scope.host_scope_count(), 2);
        let value = reenter(call, &root, function, &[]).unwrap();
        call.runtime().collect_garbage().unwrap();
        let Value::Array(array) = value.value() else {
            panic!("array")
        };
        assert_eq!(call.runtime().gc().array_get(array, 0), Some(Value::I32(7)));
    }
    for encoded in [false, true] {
        for jit in [false, true] {
            let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
            let mut security = runtime.security();
            security.profile.allow_jit = true;
            security.capabilities.jit = true;
            runtime.set_security_context(security);
            let bytecode = compile_test_bytecode(
                "fn main() -> i32 { print(\"update\"); 42 } fn compute() -> ArrayList<i32> { [7] }",
            );
            let compute = bytecode.modules[bytecode.root.index()]
                .functions
                .iter()
                .find(|f| f.name == "compute")
                .unwrap()
                .id;
            let descriptor = HostPathDescriptorId::new(0);
            let root = runtime.host().root(HostObjectId(1)).unwrap();
            let stages = Rc::new(RefCell::new(Vec::new()));
            let validation = stages.clone();
            let reading = stages.clone();
            let preparing = stages.clone();
            let write_hp = hp.clone();
            runtime
                .register_host_path_adapter(
                    descriptor,
                    HostPathAdapter::new()
                        .with_validate(move |call, _, _, _| {
                            assert_reentry(call, compute);
                            validation.borrow_mut().push("validate");
                            Ok(())
                        })
                        .with_read(move |call, _| {
                            assert_reentry(call, compute);
                            reading.borrow_mut().push("read");
                            Ok(Value::I32(10))
                        })
                        .with_prepare_write(move |call, _, record| {
                            assert_reentry(call, compute);
                            preparing.borrow_mut().push("prepare");
                            let Value::I32(next) = record.new_value else {
                                panic!("i32")
                            };
                            let hp = write_hp.clone();
                            Ok(PreparedHostPathWrite::new(move || {
                                *hp.lock().unwrap() = next
                            }))
                        }),
                )
                .unwrap();
            runtime
                .register_host_function(HostFunction::new(
                    kagari_common::host_interface::standard_log(),
                    move |call, _| {
                        call.runtime()
                            .set_host_path(
                                &Value::HostRoot(root),
                                descriptor,
                                vec![],
                                Value::I32(20),
                            )
                            .unwrap();
                        Ok(Value::Unit)
                    },
                ))
                .unwrap();
            let mut program = bytecode;
            if encoded {
                program = KbcArtifact::from_bytes(
                    &KbcArtifact::from_program(program, Default::default())
                        .unwrap()
                        .to_bytes()
                        .unwrap(),
                )
                .unwrap()
                .program;
            }
            let loaded = runtime.load_program("path-reentry.kgr", program).unwrap();
            let scope = runtime
                .begin_execution(&loaded, runtime.execution_options())
                .unwrap();
            let mut vm = Vm::new(runtime);
            let prepared = native_fixtures::unsupported();
            let report = if jit {
                vm.execute_prepared(&loaded, "main", &prepared)
            } else {
                vm.execute(&loaded, "main")
            }
            .unwrap();
            assert_eq!(report.return_value, Value::I32(42));
            assert_eq!(*hp.lock().unwrap(), 20);
            assert_eq!(*stages.borrow(), ["validate", "read", "prepare"]);
            assert_eq!(scope.host_scope_count(), 0);
            assert_eq!(scope.counters().current_call_depth, 0);
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            drop(scope);
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(vm.runtime().gc().allocated_objects(), 0);
            assert_eq!(vm.runtime().host_dirty_paths().len(), 1);
        }
    }
}

#[test]
fn executes_typed_path_read_set_modify_and_view_instructions() {
    let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
    let loaded = runtime
        .load_program(
            "paths.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![path_module(
                    &runtime,
                    "main",
                    vec![
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(0)),
                            callee: CallTarget::Native(NativeImportId::new(0)),
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
    let error = runtime
        .load_program(
            "readonly_path.kbc",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![path_module(
                    &runtime,
                    "main",
                    vec![
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(0)),
                            callee: CallTarget::Native(NativeImportId::new(0)),
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
        .unwrap_err();

    assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
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
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![path_module(
                    &runtime,
                    "main",
                    vec![
                        BytecodeInstruction::Call {
                            dst: Some(Register::new(0)),
                            callee: CallTarget::Native(NativeImportId::new(0)),
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
        VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("fs_read")
    ));
}

#[test]
fn path_calls_use_linked_slots_and_reject_missing_or_ambiguous_contracts() {
    use kagari_bytecode::artifact::KbcArtifact;
    for encoded in [false, true] {
        for jit in [false, true] {
            let (mut runtime, _) = register_vm_host_path_runtime(PathAccess::ReadWrite);
            let original = runtime
                .host()
                .path_descriptor(HostPathDescriptorId::new(0))
                .unwrap()
                .clone();
            let declaration = runtime
                .host()
                .host_type(original.root_type)
                .unwrap()
                .declaration
                .fields[0]
                .id
                .clone();
            let registration = HostPathDescriptorRegistration {
                root_type: original.root_type,
                result_type: original.result_type,
                segments: vec![HostPathSegmentRegistration::Field { declaration }],
                access: PathAccess::ReadOnly,
                schema_epoch: HostSchemaEpoch::new(0),
                capability_requirements: Default::default(),
            };
            let target = runtime
                .register_host_path_descriptor(registration.clone())
                .unwrap();
            assert_eq!(target.index(), 1);
            runtime
                .register_host_path_adapter(
                    target,
                    HostPathAdapter::new().with_read(|_, _| Ok(Value::I32(73))),
                )
                .unwrap();
            let mut bytecode = path_module(
                &runtime,
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(0)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
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
            );
            bytecode.paths[0].contract_fingerprint = runtime
                .host()
                .path_descriptor(target)
                .unwrap()
                .abi_fingerprint
                .0;
            bytecode.paths[0].read_only = true;
            bytecode.paths[0].debug_name = "deliberately unrelated diagnostic label".into();
            let program = BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![bytecode],
            };
            let mut missing = program.clone();
            missing.modules[0].paths[0].contract_fingerprint ^= 1;
            assert_eq!(
                runtime.load_program("missing", missing).unwrap_err().kind(),
                RuntimeErrorKind::TypedPathValidation
            );
            assert_eq!(runtime.modules().loaded_count(), 0);
            let program = if encoded {
                let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
                KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
                    .unwrap()
                    .program
            } else {
                program
            };
            let loaded = runtime.load_program("linked", program.clone()).unwrap();
            assert_eq!(loaded.path_binding(PathId::new(0)), Some(target));
            // Later registrations cannot retarget an already linked version.
            runtime.register_host_path_descriptor(registration).unwrap();
            assert_eq!(
                runtime
                    .load_program("ambiguous", program)
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::TypedPathValidation
            );
            assert_eq!(runtime.modules().loaded_count(), 1);
            let mut security = runtime.security();
            security.profile.allow_jit = true;
            security.capabilities.jit = true;
            runtime.set_security_context(security);
            let mut vm = Vm::new(runtime);
            let value = if jit {
                let prepared = native_fixtures::unsupported();
                vm.execute_prepared(&loaded, "main", &prepared).unwrap()
            } else {
                vm.execute(&loaded, "main").unwrap()
            };
            assert_eq!(value.return_value, Value::I32(73));
        }
    }
}

#[test]
fn path_linking_checks_dynamic_arguments_for_every_path_operation() {
    use kagari_common::host_interface::{
        path::HostIndexSegmentDeclaration, value_type::HostValueType,
    };
    let (mut runtime, _) = register_vm_host_path_runtime(PathAccess::ReadWrite);
    let field = runtime
        .host()
        .path_descriptor(HostPathDescriptorId::new(0))
        .unwrap()
        .clone();
    let target = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: field.root_type,
            result_type: field.result_type,
            segments: vec![HostPathSegmentRegistration::Index {
                declaration: HostIndexSegmentDeclaration {
                    slot: 0,
                    collection: HostValueType::opaque("game.Player"),
                    index: HostValueType::I32,
                    result: HostValueType::I32,
                    access: PathAccess::ReadWrite,
                },
            }],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
            capability_requirements: Default::default(),
        })
        .unwrap();
    let fingerprint = runtime
        .host()
        .path_descriptor(target)
        .unwrap()
        .abi_fingerprint
        .0;
    for operation in 0..4 {
        for args in [
            vec![],
            vec![Register::new(1), Register::new(1)],
            vec![Register::new(0)],
            vec![Register::new(1)],
        ] {
            let valid = args == vec![Register::new(1)];
            let instruction = match operation {
                0 => BytecodeInstruction::ReadPath {
                    dst: Register::new(2),
                    root_or_view: Register::new(0),
                    path: PathId::new(0),
                    dynamic_args: args,
                },
                1 => BytecodeInstruction::SetPath {
                    root_or_view: Register::new(0),
                    path: PathId::new(0),
                    dynamic_args: args,
                    value: Register::new(1),
                },
                2 => BytecodeInstruction::ModifyPath {
                    dst: Some(Register::new(2)),
                    root_or_view: Register::new(0),
                    path: PathId::new(0),
                    dynamic_args: args,
                    op: BinaryOp::Add,
                    value: Register::new(1),
                },
                _ => BytecodeInstruction::MakePathView {
                    dst: Register::new(5),
                    root_or_view: Register::new(0),
                    path: PathId::new(0),
                    dynamic_args: args,
                },
            };
            let mut module = path_module(
                &runtime,
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(0)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(1),
                        constant: ConstantOperand::I32(3),
                    },
                    instruction,
                    BytecodeInstruction::Return(None),
                ],
                ValueType::Unit,
            );
            module.paths[0].contract_fingerprint = fingerprint;
            let before = runtime.modules().loaded_count();
            let result = runtime.load_program(
                format!("operation{operation}"),
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![module],
                },
            );
            if valid {
                assert_eq!(result.unwrap().path_binding(PathId::new(0)), Some(target));
            } else {
                assert_eq!(
                    result.unwrap_err().kind(),
                    RuntimeErrorKind::TypedPathValidation
                );
                assert_eq!(runtime.modules().loaded_count(), before);
            }
        }
    }
}
