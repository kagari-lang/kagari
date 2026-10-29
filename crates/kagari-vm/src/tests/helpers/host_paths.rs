use super::*;
use crate::tests::common::logging_module;
use crate::{
    error::VmError,
    reentry::reenter,
    tests::{
        common::{
            compile_logging_test_bytecode, compile_native_test_bytecode, module_program,
            round_trip_bytecode,
        },
        native_fixtures,
    },
    vm::{fixtures::FixtureExecution, native::PreparedNativeEntry},
};
use kagari_abi::native_call::JIT_STATUS_ENGINE_FAULT;
use kagari_bytecode::instruction::{ConstantId, NativeImportId};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    backend::{BackendInvocationError, native::NativeInvocationFailure},
    host::{HostCallContext, HostPathDescriptorId},
    jit_abi::jit_poll_execution,
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, host::HostCallback, types::Type,
    },
};
use kagari_types::{declaration::path::HostIndexSegmentDeclaration, ty::Ty};
use std::sync::{Arc, Mutex};

#[test]
fn executes_runtime_host_helper_call() {
    let mut runtime = standard_runtime(RuntimeConfig::default());
    logging_module(unsafe {
        // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
        HostCallback::new(|_, _| Ok(Value::Unit))
    })
    .install(&mut runtime)
    .unwrap();
    let mut builder = ModuleBuilder::new("fixtures::scalar", &Default::default()).unwrap();
    let add = builder
        .define_function(
            FunctionDecl::new("add_i32")
                .parameter("lhs", Type::i32())
                .parameter("rhs", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            add,
            |_: &mut CallContext<'_>, lhs: i32, rhs: i32| -> NativeResult<i32> { Ok(lhs + rhs) },
        )
        .unwrap();
    let provider = builder.finish().unwrap();
    provider.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program(
            "helper.kbc",
            compile_native_test_bytecode(
                "use fixtures::scalar::add_i32; fn main() -> i32 { add_i32(40, 2) }",
                &[&provider],
            ),
        )
        .expect("helper module should load");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();

    let vm = Vm::new(runtime);
    let report = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn path_commit_faults_release_frames_and_prevent_further_interpreter_or_jit_execution() {
    for encoded in [false, true] {
        for jit in [false, true] {
            let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
            logging_module(unsafe {
                // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
                HostCallback::new(|_, _| Ok(Value::Unit))
            })
            .install(&mut runtime)
            .unwrap();
            let write_hp = hp.clone();
            runtime
                .register_host_path_adapter(HostPathDescriptorId::new(0), unsafe {
                    // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
                    HostPathAdapter::new()
                        .with_read_transport(|_, _| Ok(Value::I32(10)))
                        .with_prepare_write_transport(move |_, _, record| {
                            let Value::I32(value) = record.new_value else {
                                return Err(HostError::new("expected i32"));
                            };
                            let hp = write_hp.clone();
                            Ok(PreparedHostPathWrite::new(move || {
                                *hp.lock().unwrap() = value;
                                panic!("host commit violated its contract");
                            }))
                        })
                })
                .unwrap();
            let scalar = runtime
                .load_program(
                    "scalar.kgr",
                    compile_test_bytecode("fn main() -> i32 { 42 }"),
                )
                .unwrap();
            let _scalar_program = scalar;
            let scalar = runtime.select_program(&_scalar_program).unwrap();
            let prepared = native_fixtures::unsupported();
            let native = native_fixtures::install_i32::<42>(&runtime, &scalar, false);
            let scalar_entry = PreparedNativeEntry::Native(native.clone());
            let program = module_program(path_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Slot::new(0)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Slot::new(1),
                        constant: ConstantId::new(0),
                    },
                    BytecodeInstruction::SetPath {
                        root_or_view: Slot::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                        value: Slot::new(1),
                    },
                    BytecodeInstruction::Return(Some(Slot::new(1))),
                ],
                ValueType::I32,
                vec![ConstantOperand::I32(20)],
            ));
            let program = if encoded {
                round_trip_bytecode(program)
            } else {
                program
            };
            let loaded = runtime.load_program("fault.kgr", program).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let vm = Vm::new(runtime);
            let error = if jit {
                vm.execute_fixture_prepared(&loaded, FunctionRef::new(0), &prepared)
                    .unwrap_err()
            } else {
                vm.execute_fixture(&loaded, FunctionRef::new(0))
                    .unwrap_err()
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
                vm.execute(&scalar, ProgramEntry::Main).unwrap_err(),
                vm.execute_prepared(&scalar, ProgramEntry::Main, &scalar_entry)
                    .unwrap_err(),
            ] {
                assert!(
                    matches!(error, VmError::RuntimeError(ref error) if error.kind() == RuntimeErrorKind::EngineFault)
                );
            }
            assert!(matches!(vm.runtime().invoke_native_function(&native),
                Err(NativeInvocationFailure { error: BackendInvocationError::RuntimeFailure(ref error), .. }) if error.kind() == RuntimeErrorKind::EngineFault));
            assert_eq!(
                unsafe { jit_poll_execution(vm.runtime(), 0) },
                JIT_STATUS_ENGINE_FAULT
            );
            assert_eq!(vm.runtime().resources().counters(), before);
        }
    }
}

#[test]
fn typed_path_callbacks_reenter_the_root_session_before_commit() {
    fn assert_reentry(call: &HostCallContext<'_>, function: FunctionRef) {
        let root = call.runtime().execution_root().unwrap();
        let scope = call
            .runtime()
            .begin_execution(&root, call.runtime().execution_options())
            .unwrap();
        assert_eq!(scope.host_scope_count(), 2);
        let value = reenter(call, &root, function, &[]).unwrap();
        call.runtime().collect_garbage().unwrap();
        let Value::Array(array) = unsafe { value.value_transport(call.runtime().gc()) }.unwrap()
        else {
            panic!("array")
        };
        assert_eq!(
            unsafe { call.runtime().gc().array_get_transport(array, 0) },
            Some(Value::I32(7))
        );
    }
    for encoded in [false, true] {
        for jit in [false, true] {
            let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
            let bytecode = compile_logging_test_bytecode(
                "fn main() -> i32 { print(\"update\"); 42 } fn compute() -> [i32] { [7] }",
            );
            let compute = bytecode.records().modules[bytecode.records().root.index()]
                .functions
                .iter()
                .find(|f| f.name == "compute")
                .unwrap()
                .id;
            let descriptor = HostPathDescriptorId::new(0);
            let root = runtime.host().root(HostObjectId(1)).unwrap();
            let stages = Arc::new(Mutex::new(Vec::new()));
            let validation = stages.clone();
            let reading = stages.clone();
            let preparing = stages.clone();
            let write_hp = hp.clone();
            runtime
                .register_host_path_adapter(descriptor, unsafe {
                    // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
                    HostPathAdapter::new()
                        .with_validate_transport(move |call, _, _, _| {
                            assert_reentry(call, compute);
                            validation.lock().unwrap().push("validate");
                            Ok(())
                        })
                        .with_read_transport(move |call, _| {
                            assert_reentry(call, compute);
                            reading.lock().unwrap().push("read");
                            Ok(Value::I32(10))
                        })
                        .with_prepare_write_transport(move |call, _, record| {
                            assert_reentry(call, compute);
                            preparing.lock().unwrap().push("prepare");
                            let Value::I32(next) = record.new_value else {
                                panic!("i32")
                            };
                            let hp = write_hp.clone();
                            Ok(PreparedHostPathWrite::new(move || {
                                *hp.lock().unwrap() = next
                            }))
                        })
                })
                .unwrap();
            logging_module(unsafe {
                // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
                HostCallback::new(move |call, _| {
                    call.runtime()
                        .set_host_path_transport(
                            &call.runtime().gc().alloc_host_root_transport(root).unwrap(),
                            descriptor,
                            vec![],
                            Value::I32(20),
                        )
                        .unwrap();
                    Ok(Value::Unit)
                })
            })
            .install(&mut runtime)
            .unwrap();
            let mut program = bytecode;
            if encoded {
                program = round_trip_bytecode(program);
            }
            let loaded = runtime.load_program("path-reentry.kgr", program).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let vm = Vm::new(runtime);
            let scope = vm
                .runtime()
                .begin_execution(&loaded, vm.runtime().execution_options())
                .unwrap();
            let prepared = native_fixtures::unsupported();
            let report = if jit {
                vm.execute_prepared(&loaded, ProgramEntry::Main, &prepared)
            } else {
                vm.execute(&loaded, ProgramEntry::Main)
            }
            .unwrap();
            assert_eq!(
                report
                    .return_value
                    .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                    .expect("retained execution result"),
                Value::I32(42)
            );
            assert_eq!(*hp.lock().unwrap(), 20);
            assert_eq!(*stages.lock().unwrap(), ["validate", "read", "prepare"]);
            assert_eq!(scope.host_scope_count(), 0);
            assert_eq!(scope.counters().current_call_depth, 0);
            drop(report);
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            drop(scope);
            let replacement = vm
                .runtime()
                .stage_reload_verified_program(
                    &_loaded_program,
                    None,
                    loaded.verified_program().clone(),
                )
                .unwrap();
            vm.runtime().publish_staged_reload(replacement).unwrap();
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(vm.runtime().gc().allocated_objects(), 0);
            assert_eq!(
                unsafe { vm.runtime().host_dirty_paths_transport() }.len(),
                1
            );
        }
    }
}

#[test]
fn executes_typed_path_read_set_modify_and_view_instructions() {
    let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
    let loaded = runtime
        .load_program(
            "paths.kbc",
            module_program(path_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Slot::new(0)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![],
                    },
                    BytecodeInstruction::ReadPath {
                        dst: Slot::new(1),
                        root_or_view: Slot::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Slot::new(2),
                        constant: ConstantId::new(0),
                    },
                    BytecodeInstruction::SetPath {
                        root_or_view: Slot::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                        value: Slot::new(2),
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Slot::new(3),
                        constant: ConstantId::new(1),
                    },
                    BytecodeInstruction::ModifyPath {
                        dst: Some(Slot::new(4)),
                        root_or_view: Slot::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                        op: BinaryOp::Add,
                        value: Slot::new(3),
                    },
                    BytecodeInstruction::MakePathView {
                        dst: Slot::new(5),
                        root_or_view: Slot::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                    },
                    BytecodeInstruction::Return(Some(Slot::new(4))),
                ],
                ValueType::I32,
                vec![ConstantOperand::I32(5), ConstantOperand::I32(2)],
            )),
        )
        .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();

    let vm = Vm::new(runtime);
    let report = vm.execute_fixture(&loaded, FunctionRef::new(0)).unwrap();

    assert_eq!(
        report
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(7)
    );
    assert_eq!(*hp.lock().unwrap(), 7);
    assert_eq!(
        unsafe { vm.runtime().host_dirty_paths_transport() }.len(),
        2
    );
}

#[test]
fn typed_path_instruction_failures_are_runtime_typed_path_errors() {
    let (mut runtime, _) = register_vm_host_path_runtime(PathAccess::ReadOnly);
    let error = runtime
        .load_program(
            "readonly_path.kbc",
            module_program(path_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Slot::new(0)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Slot::new(1),
                        constant: ConstantId::new(0),
                    },
                    BytecodeInstruction::SetPath {
                        root_or_view: Slot::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                        value: Slot::new(1),
                    },
                    BytecodeInstruction::Return(None),
                ],
                ValueType::Unit,
                vec![ConstantOperand::I32(2)],
            )),
        )
        .unwrap_err();

    assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
}

#[test]
fn installed_typed_path_helpers_need_no_permission_flags() {
    let (mut runtime, _) = register_vm_host_path_runtime(PathAccess::ReadWrite);
    let loaded = runtime
        .load_program(
            "path_capability.kbc",
            module_program(path_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Slot::new(0)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![],
                    },
                    BytecodeInstruction::ReadPath {
                        dst: Slot::new(1),
                        root_or_view: Slot::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                    },
                    BytecodeInstruction::Return(Some(Slot::new(1))),
                ],
                ValueType::I32,
                vec![],
            )),
        )
        .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();

    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute_fixture(&loaded, FunctionRef::new(0))
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(10)
    );
}

#[test]
fn path_calls_use_linked_slots_and_reject_missing_or_ambiguous_contracts() {
    for encoded in [false, true] {
        for jit in [false, true] {
            let (mut runtime, _) = register_vm_host_path_runtime(PathAccess::ReadWrite);
            let original = runtime
                .host()
                .path_descriptor(HostPathDescriptorId::new(0))
                .unwrap()
                .clone();
            let HostPathSegmentDeclaration::Field(declaration) = original.declaration.segments[0]
            else {
                panic!("fixture requires the hp field path");
            };
            let registration = HostPathDescriptorRegistration {
                definitions: runtime.host().definitions(),
                root_type: original.root_type,
                result_type: original.result_type,
                segments: vec![HostPathSegmentDeclaration::Field(declaration)],
                access: PathAccess::ReadOnly,
                schema_epoch: HostSchemaEpoch::new(0),
            };
            let target = runtime
                .register_host_path_descriptor(registration.clone())
                .unwrap();
            assert_eq!(target.index(), 1);
            runtime
                .register_host_path_adapter(target, unsafe {
                    // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
                    HostPathAdapter::new().with_read_transport(|_, _| Ok(Value::I32(73)))
                })
                .unwrap();
            let bytecode = path_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Slot::new(0)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![],
                    },
                    BytecodeInstruction::ReadPath {
                        dst: Slot::new(1),
                        root_or_view: Slot::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                    },
                    BytecodeInstruction::Return(Some(Slot::new(1))),
                ],
                ValueType::I32,
                vec![],
            );
            let required = runtime
                .host()
                .path_descriptor(target)
                .unwrap()
                .declaration
                .clone();
            let required = DefinitionMetadata::checked(
                runtime.host().definitions(),
                required,
                &Default::default(),
            )
            .unwrap();
            let (definitions, mut bytecode) = with_path_contract(bytecode, required).into_parts();
            bytecode.paths[0].debug_name = "deliberately unrelated diagnostic label".into();
            let bytecode =
                DefinitionMetadata::checked(definitions.clone(), bytecode, &Default::default())
                    .unwrap();
            // A valid portable contract with an uninstalled epoch tests runtime linking.
            let mut missing_path = bytecode.records().path_declarations[0].clone();
            missing_path.schema_epoch += 1;
            let missing_path =
                DefinitionMetadata::checked(definitions, missing_path, &Default::default())
                    .unwrap();
            let missing = module_program(with_path_contract(bytecode.clone(), missing_path));
            let program = module_program(bytecode);
            assert_eq!(
                runtime.load_program("missing", missing).unwrap_err().kind(),
                RuntimeErrorKind::TypedPathValidation
            );
            assert_eq!(runtime.modules().loaded_count(), 0);
            let program = if encoded {
                round_trip_bytecode(program)
            } else {
                program
            };
            let loaded = runtime.load_program("linked", program.clone()).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
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
            let vm = Vm::new(runtime);
            let value = if jit {
                let prepared = native_fixtures::unsupported();
                vm.execute_fixture_prepared(&loaded, FunctionRef::new(0), &prepared)
                    .unwrap()
            } else {
                vm.execute_fixture(&loaded, FunctionRef::new(0)).unwrap()
            };
            assert_eq!(
                value
                    .return_value
                    .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                    .expect("retained execution result"),
                Value::I32(73)
            );
        }
    }
}

#[test]
fn path_linking_checks_dynamic_arguments_for_every_path_operation() {
    let (mut runtime, _) = register_vm_host_path_runtime(PathAccess::ReadWrite);
    let field = runtime
        .host()
        .path_descriptor(HostPathDescriptorId::new(0))
        .unwrap()
        .clone();
    let target = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            definitions: runtime.host().definitions(),
            root_type: field.root_type,
            result_type: field.result_type,
            segments: vec![HostPathSegmentDeclaration::Index(
                HostIndexSegmentDeclaration {
                    slot: 0,
                    collection: Ty::Host(field.declaration.root),
                    index: Ty::Builtin(BuiltinType::I32),
                    result: Ty::Builtin(BuiltinType::I32),
                    access: PathAccess::ReadWrite,
                },
            )],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
        })
        .unwrap();
    runtime
        .register_host_path_adapter(target, unsafe {
            // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
            HostPathAdapter::new().with_read_transport(|_, _| Ok(Value::I32(0)))
        })
        .unwrap();
    let required = DefinitionMetadata::checked(
        runtime.host().definitions(),
        runtime
            .host()
            .path_descriptor(target)
            .unwrap()
            .declaration
            .clone(),
        &Default::default(),
    )
    .unwrap();
    for operation in 0..4 {
        for args in [
            vec![],
            vec![Slot::new(1), Slot::new(1)],
            vec![Slot::new(0)],
            vec![Slot::new(1)],
        ] {
            let valid = args == vec![Slot::new(1)];
            let instruction = match operation {
                0 => BytecodeInstruction::ReadPath {
                    dst: Slot::new(2),
                    root_or_view: Slot::new(0),
                    path: PathId::new(0),
                    dynamic_args: args,
                },
                1 => BytecodeInstruction::SetPath {
                    root_or_view: Slot::new(0),
                    path: PathId::new(0),
                    dynamic_args: args,
                    value: Slot::new(1),
                },
                2 => BytecodeInstruction::ModifyPath {
                    dst: Some(Slot::new(2)),
                    root_or_view: Slot::new(0),
                    path: PathId::new(0),
                    dynamic_args: args,
                    op: BinaryOp::Add,
                    value: Slot::new(1),
                },
                _ => BytecodeInstruction::MakePathView {
                    dst: Slot::new(5),
                    root_or_view: Slot::new(0),
                    path: PathId::new(0),
                    dynamic_args: args,
                },
            };
            let module = path_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Slot::new(0)),
                        callee: CallTarget::Native(NativeImportId::new(0)),
                        args: vec![],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Slot::new(1),
                        constant: ConstantId::new(0),
                    },
                    instruction,
                    BytecodeInstruction::Return(None),
                ],
                ValueType::Unit,
                vec![ConstantOperand::I32(3)],
            );
            let module = with_path_contract(module, required.clone());
            let before = runtime.modules().loaded_count();
            let result =
                runtime.load_program(format!("operation{operation}"), module_program(module));
            if valid {
                let selected = runtime.select_program(&result.unwrap()).unwrap();
                assert_eq!(selected.path_binding(PathId::new(0)), Some(target));
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
