use super::*;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    backend::{BackendInvocationError, native::NativeInvocationFailure},
    host::{HostCallContext, HostPathDescriptorId},
    jit_abi::jit_poll_execution,
    module::entry::ProgramEntry,
    native::{
        conversion::{arguments::ArgumentValues, context::ConversionContext},
        host::callback,
    },
    value::{carriers::RootedValue, view::ValueView},
};
use crate::{
    execution::{fixtures::FixtureExecution, native::PreparedNativeEntry, reentry::reenter},
    interpreter_tests::{
        common::{
            compile_native_test_bytecode, logging_module, module_program, round_trip_bytecode,
        },
        native_fixtures,
    },
};
use kagari_abi::native_call::JIT_STATUS_ENGINE_FAULT;
use kagari_bytecode::instruction::{ConstantId, NativeImportId};
use kagari_contract::ids::FunctionRef;
use kagari_types::{declaration::path::HostIndexSegmentDeclaration, ty::Ty};
use std::sync::{Arc, Mutex};

#[test]
fn path_commit_faults_release_frames_and_prevent_further_interpreter_or_jit_execution() {
    for encoded in [false, true] {
        for jit in [false, true] {
            let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
            logging_module(|_, _| Ok(ValueView::unit()))
                .install(&mut runtime)
                .unwrap();
            let write_hp = hp.clone();
            runtime
                .register_host_path_adapter(
                    HostPathDescriptorId::new(0),
                    HostPathAdapter::new()
                        .with_read(|_, _| Ok(ValueView::from(10i32)))
                        .with_prepare_write(move |_, _, record| {
                            let Some(value) = record.new_value.as_i32() else {
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
            let scalar = load_bytecode(
                &mut runtime,
                "scalar.kgr",
                compile_test_bytecode(include_str!("../../../tests/fixtures/host/path-scalar.kgr")),
            )
            .unwrap();
            let _scalar_program = scalar;
            let scalar = runtime.select_program(&_scalar_program).unwrap();
            let prepared = native_fixtures::unsupported();
            let native = native_fixtures::install_i32::<42>(&mut runtime, &scalar, false);
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
            let loaded = load_bytecode(&mut runtime, "fault.kgr", program).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let roots = runtime.collected_gc_stats().root_registrations;
            let mut vm = runtime;
            let error = if jit {
                vm.execute_fixture_prepared(&loaded, FunctionRef::new(0), &prepared)
                    .unwrap_err()
            } else {
                vm.execute_fixture(&loaded, FunctionRef::new(0))
                    .unwrap_err()
            };
            assert!(matches!(error, ref error if error.kind() == RuntimeErrorKind::EngineFault));
            assert!(vm.is_quarantined());
            assert_eq!(vm.modules().retention_counts(loaded.key()).active_calls, 0);
            assert_eq!(*hp.lock().unwrap(), 20); // Internal faults do not promise business rollback.
            assert_eq!(vm.collected_gc_stats().root_registrations, roots);
            assert_eq!(vm.resources().counters().current_call_depth, 0);
            let before = vm.resources().counters();
            for error in [
                vm.execute_module(&scalar, ProgramEntry::Main, vm.execution_options())
                    .unwrap_err(),
                vm.execute_prepared_module(
                    &scalar,
                    ProgramEntry::Main,
                    &scalar_entry,
                    vm.execution_options(),
                )
                .unwrap_err(),
            ] {
                assert!(
                    matches!(error, ref error if error.kind() == RuntimeErrorKind::EngineFault)
                );
            }
            assert!(matches!(vm.invoke_native_function(&native),
                Err(NativeInvocationFailure { error: BackendInvocationError::RuntimeFailure(ref error), .. }) if error.kind() == RuntimeErrorKind::EngineFault));
            assert_eq!(
                unsafe { jit_poll_execution(&mut vm, 0) },
                JIT_STATUS_ENGINE_FAULT
            );
            assert_eq!(vm.resources().counters(), before);
        }
    }
}

#[test]
fn typed_path_callbacks_reenter_the_root_session_before_commit() {
    fn assert_reentry(call: &mut HostCallContext<'_>, function: FunctionRef) {
        let root = call.runtime().execution_root().unwrap();
        let options = call.runtime().execution_options();
        let scope = call.runtime_mut().begin_execution(&root, options).unwrap();
        assert_eq!(scope.host_scope_count(), 2);
        drop(scope);
        let roots = call.runtime_mut().collected_gc_stats().root_registrations;
        let value = reenter(call, &root, function, ArgumentValues::scalars(&[])).unwrap();
        call.runtime_mut().collect_garbage().unwrap();
        {
            let mut cx = ConversionContext::new(call.runtime_mut(), &root).unwrap();
            let array = cx.import(&value).unwrap();
            assert_eq!(cx.array_len(&array).unwrap(), 1);
            assert_eq!(
                cx.array_element(&array, 0).unwrap().unwrap().as_i32(),
                Some(7)
            );
        }
        drop(value);
        assert_eq!(
            call.runtime_mut().collected_gc_stats().root_registrations,
            roots
        );
    }
    for encoded in [false, true] {
        for jit in [false, true] {
            let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
            let descriptor = HostPathDescriptorId::new(0);
            let root = runtime.host().root(HostObjectId(1)).unwrap();
            let mut entered = ModuleBuilder::new("fixture::effects", &Default::default()).unwrap();
            let entry = entered
                .define_function(FunctionDecl::new("entered").host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_trap: true,
                        may_call_host_services: true,
                        may_mutate_host_state: true,
                        ..Default::default()
                    },
                }))
                .unwrap();
            entered
                .bind(
                    entry,
                    callback(move |call: &mut HostCallContext<'_>, _| {
                        let root_value = call.runtime_mut().host_root_value(root).unwrap();
                        call.runtime_mut()
                            .set_host_path(&root_value, descriptor, &[], &RootedValue::from(20i32))
                            .unwrap();
                        Ok(ValueView::unit())
                    }),
                )
                .unwrap();
            let entered = entered.finish().unwrap();
            let bytecode = compile_native_test_bytecode(
                include_str!("../../../tests/fixtures/host/path-reentry.kgr"),
                &[&entered],
            );
            let compute = bytecode.records().modules[bytecode.records().root.index()]
                .functions
                .iter()
                .find(|f| f.name == "compute")
                .unwrap()
                .id;
            let stages = Arc::new(Mutex::new(Vec::new()));
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
                            validation.lock().unwrap().push("validate");
                            Ok(())
                        })
                        .with_read(move |call, _| {
                            assert_reentry(call, compute);
                            reading.lock().unwrap().push("read");
                            Ok(ValueView::from(10i32))
                        })
                        .with_prepare_write(move |call, _, record| {
                            assert_reentry(call, compute);
                            preparing.lock().unwrap().push("prepare");
                            let Some(next) = record.new_value.as_i32() else {
                                panic!("i32")
                            };
                            let hp = write_hp.clone();
                            Ok(PreparedHostPathWrite::new(move || {
                                *hp.lock().unwrap() = next
                            }))
                        }),
                )
                .unwrap();
            entered.install(&mut runtime).unwrap();
            let mut program = bytecode;
            if encoded {
                program = round_trip_bytecode(program);
            }
            let loaded = load_bytecode(&mut runtime, "path-reentry.kgr", program).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            runtime.collect_garbage().unwrap();
            let baseline = runtime.gc_stats();
            let mut vm = runtime;
            let mut scope = vm.begin_execution(&loaded, vm.execution_options()).unwrap();
            let prepared = native_fixtures::unsupported();
            let options = scope.runtime().execution_options();
            let report = if jit {
                scope.runtime_mut().execute_prepared_module(
                    &loaded,
                    ProgramEntry::Main,
                    &prepared,
                    options,
                )
            } else {
                scope
                    .runtime_mut()
                    .execute_module(&loaded, ProgramEntry::Main, options)
            }
            .unwrap();
            assert_eq!(report.return_value.as_i32(), Some(42));
            assert_eq!(*hp.lock().unwrap(), 20);
            assert_eq!(*stages.lock().unwrap(), ["validate", "read", "prepare"]);
            assert_eq!(scope.host_scope_count(), 0);
            assert_eq!(scope.counters().current_call_depth, 0);
            drop(report);
            assert_eq!(
                scope.runtime_mut().collected_gc_stats().root_registrations,
                baseline.root_registrations
            );
            drop(scope);
            let replacement = vm
                .stage_reload_verified_program(
                    &_loaded_program,
                    None,
                    loaded.verified_program().clone(),
                )
                .unwrap();
            vm.publish_staged_reload(replacement).unwrap();
            vm.collect_garbage().unwrap();
            assert_eq!(vm.gc_stats().objects, baseline.objects);
            assert_eq!(
                vm.collected_gc_stats().root_registrations,
                baseline.root_registrations
            );
            assert_eq!(vm.host_dirty_paths().len(), 1);
        }
    }
}

#[test]
fn executes_typed_path_read_set_modify_and_view_instructions() {
    let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
    let loaded = load_bytecode(
        &mut runtime,
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

    let mut vm = runtime;
    let report = vm.execute_fixture(&loaded, FunctionRef::new(0)).unwrap();

    assert_eq!(report.return_value.as_i32(), Some(7));
    assert_eq!(*hp.lock().unwrap(), 7);
    assert_eq!(vm.host_dirty_paths().len(), 2);
}

#[test]
fn typed_path_instruction_failures_are_runtime_typed_path_errors() {
    let (mut runtime, _) = register_vm_host_path_runtime(PathAccess::ReadOnly);
    let error = load_bytecode(
        &mut runtime,
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
    let loaded = load_bytecode(
        &mut runtime,
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

    let mut vm = runtime;
    assert_eq!(
        vm.execute_fixture(&loaded, FunctionRef::new(0))
            .unwrap()
            .return_value
            .as_i32(),
        Some(10)
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
                .register_host_path_adapter(
                    target,
                    HostPathAdapter::new().with_read(|_, _| Ok(ValueView::from(73i32))),
                )
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
                load_bytecode(&mut runtime, "missing", missing)
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::TypedPathValidation
            );
            assert_eq!(runtime.modules().loaded_count(), 0);
            let program = if encoded {
                round_trip_bytecode(program)
            } else {
                program
            };
            let loaded = load_bytecode(&mut runtime, "linked", program.clone()).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            assert_eq!(loaded.path_binding(PathId::new(0)), Some(target));
            // Later registrations cannot retarget an already linked version.
            runtime.register_host_path_descriptor(registration).unwrap();
            assert_eq!(
                load_bytecode(&mut runtime, "ambiguous", program)
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::TypedPathValidation
            );
            assert_eq!(runtime.modules().loaded_count(), 1);
            let mut vm = runtime;
            let value = if jit {
                let prepared = native_fixtures::unsupported();
                vm.execute_fixture_prepared(&loaded, FunctionRef::new(0), &prepared)
                    .unwrap()
            } else {
                vm.execute_fixture(&loaded, FunctionRef::new(0)).unwrap()
            };
            assert_eq!(value.return_value.as_i32(), Some(73));
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
        .register_host_path_adapter(
            target,
            HostPathAdapter::new().with_read(|_, _| Ok(ValueView::from(0i32))),
        )
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
            let result = load_bytecode(
                &mut runtime,
                format!("operation{operation}"),
                module_program(module),
            );
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
