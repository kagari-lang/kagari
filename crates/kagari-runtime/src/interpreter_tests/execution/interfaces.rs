use crate::native::interfaces::Interface;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;

use super::*;
use crate::{
    execution::fixtures::FixtureExecution,
    executor::Executor,
    interpreter_tests::common::{round_trip_bytecode, test_runtime},
    native::conversion::{arguments::ArgumentValues, context::ConversionContext},
    value::{carriers::RootedValue, kind::ObjectKind, view::ValueView},
};
use kagari_bytecode::instruction::ConstructionTarget as BytecodeConstruction;
use kagari_bytecode::{
    instruction::{ConstantId, StructId},
    program::{identity, verify_program},
    verifier::{BytecodeVerificationError, verify_module},
};

use crate::{
    Runtime, RuntimeConfig,
    frame::{
        driver::{ExecutionAction, ReturnAction},
        transfer::ReturnValue,
    },
    module::{ExecutableModule, entry::ProgramEntry},
};
use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_types::ty::{NominalTy, Ty};
use std::{collections::HashSet, slice};

#[test]
fn concrete_interface_object_resolves_a_linked_method_slot() {
    let (mut runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/interface-tag.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let table = &loaded.bytecode.interface_tables[0];
    let method = table.methods[0].method;
    let boxed = runtime
        .make_interface(&loaded, 0, &RootedValue::from(7i32))
        .unwrap();
    let resolved = runtime.resolve_interface_method(&boxed, &method).unwrap();
    assert_eq!(resolved.receiver(&mut runtime).unwrap().as_i32(), Some(7));
    assert_eq!(
        resolved.implementation(&mut runtime).unwrap().key(),
        loaded.key()
    );
    assert_eq!(
        resolved.target(&mut runtime).unwrap(),
        table.methods[0].target
    );
    let mut foreign = Runtime::default();
    assert!(resolved.implementation(&mut foreign).is_err());
    assert!(resolved.target(&mut foreign).is_err());
    assert!(resolved.parameter_types(&mut foreign).is_err());
    assert!(resolved.return_type(&mut foreign).is_err());
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.interface_description(&boxed).unwrap().concrete_type,
        Ty::Builtin(BuiltinType::I32)
    );
    assert!(
        runtime
            .resolve_interface_method(&RootedValue::from(7i32), &method)
            .is_err()
    );
    assert!(
        runtime
            .validate_interface_method_result(&resolved, &RootedValue::from(true))
            .is_err()
    );

    let mut vm = runtime;
    assert_eq!(
        vm.invoke_interface_method(&boxed, &method, &[])
            .unwrap()
            .as_i32(),
        Some(8)
    );
    assert!(
        vm.invoke_interface_method(&boxed, &method, &[RootedValue::from(true)])
            .is_err()
    );
}

#[test]
fn interface_method_slots_follow_trait_order_even_when_impl_order_differs() {
    let (mut runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/interface-slot-order.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let table = loaded
        .bytecode
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .unwrap();
    let Ty::Trait(interface) = &table.trait_type else {
        panic!("expected trait interface")
    };
    let boxed = runtime
        .make_interface(&loaded, 0, &RootedValue::from(7i32))
        .unwrap();
    let first = runtime
        .resolve_interface_method_slot(&boxed, interface, 0, &[])
        .unwrap();
    let second = runtime
        .resolve_interface_method_slot(&boxed, interface, 1, &[])
        .unwrap();
    let method = |name| {
        loaded.bytecode.interface_tables[0]
            .methods
            .iter()
            .find(|slot| {
                loaded
                    .definitions()
                    .resolve(slot.method)
                    .unwrap()
                    .segments()
                    .last()
                    .unwrap()
                    .name
                    == name
            })
            .unwrap()
            .method
    };
    assert_eq!(
        first.target(&mut runtime).unwrap(),
        runtime
            .resolve_interface_method(&boxed, &method("first"))
            .unwrap()
            .target(&mut runtime)
            .unwrap()
    );
    assert_eq!(
        second.target(&mut runtime).unwrap(),
        runtime
            .resolve_interface_method(&boxed, &method("second"))
            .unwrap()
            .target(&mut runtime)
            .unwrap()
    );
    assert_ne!(
        first.target(&mut runtime).unwrap(),
        second.target(&mut runtime).unwrap()
    );
    assert!(
        runtime
            .resolve_interface_method_slot(&boxed, interface, 2, &[])
            .is_err()
    );
    let mut wrong = interface.clone();
    let mut path = loaded
        .definitions()
        .resolve(wrong.declaration)
        .unwrap()
        .to_path();
    path.path.last_mut().unwrap().name = "Other".into();
    wrong.declaration = loaded.definitions().lookup(&path).unwrap();
    assert!(
        runtime
            .resolve_interface_method_slot(&boxed, &wrong, 0, &[])
            .is_err()
    );
}

#[test]
fn source_call_boxes_a_concrete_argument_for_an_interface_parameter() {
    let (runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/interface-argument.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    assert!(
        loaded
            .bytecode
            .functions
            .iter()
            .flat_map(|function| &function.instructions)
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeInterface { .. }))
    );
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn source_call_boxes_an_interface_with_methods() {
    let (runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/interface-argument-methods.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn source_interface_method_call_dispatches_through_the_linked_slot() {
    let (runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/interface-dispatch.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(8)
    );
}

#[test]
fn static_bound_calls_preserve_explicit_self_typed_arguments_and_checked_signatures() {
    let source = include_str!("../../../tests/fixtures/execution/interface-static-bound.kgr");
    let program = round_trip_bytecode(compile_test_bytecode(source));
    let (definitions, records) = program.clone().into_parts();
    for corruption in 0..3 {
        let mut forged = records.clone();
        let (dst, args) = forged
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Call {
                    dst,
                    callee: CallTarget::InterfaceMethod { contract, .. },
                    args,
                } if contract.receiver.is_some() && args.len() == 2 => Some((dst, args)),
                _ => None,
            })
            .expect("shared static call with an explicit Self-typed argument");
        match corruption {
            0 => {
                args.pop();
            }
            1 => args[0] = args[1],
            2 => *dst = Some(args[1]),
            _ => unreachable!(),
        }
        let error = verify_program(&forged, &definitions, &Default::default()).unwrap_err();
        assert!(
            matches!(
                error,
                BytecodeVerificationError::InvalidOperation { .. }
                    | BytecodeVerificationError::TypeMismatch { .. }
            ),
            "corruption {corruption}: {error:?}"
        );
    }
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = test_runtime(config);
    let loaded = load_bytecode(&mut runtime, "static-bound", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let objects = runtime.gc_stats().objects;
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert_eq!(vm.gc_stats().root_registrations, roots);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, objects);
}

#[test]
fn source_return_and_local_bindings_keep_the_boxed_interface_value() {
    let (runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/interface-return.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn interface_method_selects_current_implementation_after_reload() {
    let source = include_str!("../../../tests/fixtures/execution/interface-tag.kgr");
    let first = compile_test_bytecode(source);
    let second = compile_test_bytecode(&source.replace("self + 1", "self + 2"));
    let mut runtime = test_runtime(Default::default());
    let old = load_bytecode(&mut runtime, "interface-reload", first).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method;
    let old_value = runtime
        .make_interface(&old, 0, &RootedValue::from(7i32))
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&_old_program, Some("interface-reload".into()), second)
        .unwrap();
    let new = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let _new_program = _old_program.clone();
    let new_value = runtime
        .make_interface(&new, 0, &RootedValue::from(7i32))
        .unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.invoke_interface_method(&old_value, &method, &[])
            .unwrap()
            .as_i32(),
        Some(9)
    );
    assert_eq!(
        vm.invoke_interface_method(&new_value, &method, &[])
            .unwrap()
            .as_i32(),
        Some(9)
    );
    drop(old_value);
    vm.collect_garbage().unwrap();
    assert!(vm.modules().loaded(old.key()).is_none());
}

#[test]
fn interface_frames_and_descendants_select_current_code() {
    let source = include_str!("../../../tests/fixtures/execution/interface-frame-reload.kgr");
    let old_code = compile_test_bytecode(source);
    let new_code = compile_test_bytecode(&source.replace(
        "fn helper() -> i32 {\n    1\n}",
        "fn helper() -> i32 {\n    2\n}",
    ));
    let mut runtime = test_runtime(Default::default());
    let old = load_bytecode(&mut runtime, "interface-frames", old_code).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method;
    let boxed = runtime
        .make_interface(&old, 0, &RootedValue::from(7i32))
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&_old_program, Some("interface-frames".into()), new_code)
        .unwrap();
    let new = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let _new_program = _old_program.clone();
    let resolved = runtime.resolve_interface_method(&boxed, &method).unwrap();
    let entry = new
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .id;
    let helper = old
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "helper")
        .unwrap()
        .id;
    let mut stack_activation = runtime.enter_execution_stack(&new).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack.push(runtime, new.slot(), entry, &[], None).unwrap();
            let wrong = runtime.resolve_interface_method(&boxed, &method).unwrap();
            assert!(
                stack
                    .push_interface_method(
                        runtime,
                        &wrong,
                        ArgumentValues::scalars(&[ValueView::from(true)]),
                        None
                    )
                    .is_err()
            );
            drop(wrong);
            assert_eq!(stack.current(runtime).unwrap().loaded().key(), new.key());
            stack
                .push_interface_method(runtime, &resolved, ArgumentValues::scalars(&[]), None)
                .unwrap();
            // The callee must retain selection metadata after its host lease and the
            // receiver's external root are gone, even while a newer program is current.
            drop((resolved, boxed));
            runtime.collect_garbage().unwrap();
            assert_eq!(stack.current(runtime).unwrap().loaded().key(), new.key());
            stack.push(runtime, new.slot(), helper, &[], None).unwrap();
            assert_eq!(stack.current(runtime).unwrap().loaded().key(), new.key());
            stack.pop(runtime).unwrap();
            assert!(matches!(
                stack
                    .finish_return(runtime, ReturnValue::scalar(ValueType::I32, 8))
                    .unwrap(),
                ReturnAction::Continue(ExecutionAction::Script)
            ));
            assert_eq!(stack.current(runtime).unwrap().loaded().key(), new.key());
            stack.pop(runtime).unwrap();
            assert_eq!(runtime.resources().counters().current_call_depth, 0);

            Ok(())
        })
        .unwrap();
}

#[test]
fn source_interface_dispatch_selects_current_method_and_descendant_after_reload() {
    let source = include_str!("../../../tests/fixtures/execution/interface-dispatch-reload.kgr");
    let old_code = compile_test_bytecode(source);
    let new_code = compile_test_bytecode(&source.replace(
        "fn helper() -> i32 {\n    1\n}",
        "fn helper() -> i32 {\n    2\n}",
    ));
    let mut runtime = test_runtime(Default::default());
    let old = load_bytecode(&mut runtime, "interface-dispatch-reload", old_code).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let old_value = runtime
        .make_interface(&old, 0, &RootedValue::from(7i32))
        .unwrap();
    let candidate = runtime
        .stage_reload_program(
            &_old_program,
            Some("interface-dispatch-reload".into()),
            new_code,
        )
        .unwrap();
    let new = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let _new_program = _old_program.clone();
    let new_value = runtime
        .make_interface(&new, 0, &RootedValue::from(7i32))
        .unwrap();
    let read = new
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "read")
        .unwrap()
        .id;
    for value in [&old_value, &new_value] {
        let mut conversion = ConversionContext::new(&mut runtime, &new).unwrap();
        let args = conversion.root_arguments(slice::from_ref(value)).unwrap();
        let result = Executor::new(conversion.runtime_mut(), &new, read, args)
            .unwrap()
            .run(|runtime, stack, value| stack.retain_result(runtime, value))
            .unwrap();
        assert_eq!(result.as_i32(), Some(9));
    }
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    runtime.collect_garbage().unwrap();
    assert!(
        old.members()
            .all(|member| runtime.modules().loaded(member.key()).is_some())
    );
    drop(old_value);
    let expected: HashSet<_> = old.members().map(|member| member.key()).collect();
    runtime.collect_garbage().unwrap();
    assert!(
        expected
            .iter()
            .all(|key| runtime.modules().loaded(*key).is_none())
    );
}

#[test]
fn generic_interface_and_retained_closure_keep_their_environment_after_reload() {
    let source = include_str!("../../../tests/fixtures/execution/generic-interface-capture.kgr");
    let mut runtime = test_runtime(Default::default());
    let old = load_bytecode(&mut runtime, "shared-reload", compile_test_bytecode(source)).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let interface = runtime
        .make_interface(&old, 0, &RootedValue::from(7i32))
        .unwrap();
    let make = old
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "make")
        .unwrap()
        .id;
    let closure = {
        let mut execution =
            Executor::new(&mut runtime, &old, make, ArgumentValues::scalars(&[])).unwrap();
        execution
            .run(|runtime, stack, value| stack.retain_result(runtime, value))
            .unwrap()
    };
    assert_eq!(source.matches("42").count(), 1);
    let candidate = runtime
        .stage_reload_program(
            &_old_program,
            Some("shared-reload".into()),
            compile_test_bytecode(&source.replace("42", "43")),
        )
        .unwrap();
    let new = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let _new_program = _old_program.clone();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old.key()).is_some());
    for (name, args, expected) in [
        ("read", vec![interface.clone()], 43),
        ("use_closure", vec![closure.clone()], 43),
        ("main", vec![], 43),
    ] {
        let entry = new
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap()
            .id;
        let mut conversion = ConversionContext::new(&mut runtime, &new).unwrap();
        let args = conversion.root_arguments(&args).unwrap();
        let result = Executor::new(conversion.runtime_mut(), &new, entry, args)
            .unwrap()
            .run(|runtime, stack, value| stack.retain_result(runtime, value))
            .unwrap();
        assert_eq!(result.as_i32(), Some(expected));
    }
    drop((interface, closure));
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    // Retire linked construction caches along with their owning publication.
    let candidate = runtime
        .stage_reload_program(
            &_old_program,
            None,
            compile_test_bytecode(&source.replace("42", "43")),
        )
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old.key()).is_none());
}

#[test]
fn trapped_interface_frame_releases_its_roots_and_call_depth() {
    let (mut runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/interface-trap.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let method = loaded.bytecode.interface_tables[0].methods[0].method;
    let boxed = runtime
        .make_interface(&loaded, 0, &RootedValue::from(7i32))
        .unwrap();
    let mut vm = runtime;
    assert!(vm.invoke_interface_method(&boxed, &method, &[]).is_err());
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn interface_method_rejects_wrong_nominal_argument_before_execution() {
    let (mut runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/interface-nominal-argument.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let method = loaded.bytecode.interface_tables[0].methods[0].method;
    let boxed = runtime
        .make_interface(&loaded, 0, &RootedValue::from(7i32))
        .unwrap();
    let mut vm = runtime;
    let right = vm
        .execute_module(&loaded, "make_a", vm.execution_options())
        .unwrap()
        .return_value;
    assert_eq!(
        vm.invoke_interface_method(&boxed, &method, &[right])
            .unwrap()
            .as_i32(),
        Some(5)
    );
    let wrong = vm
        .execute_module(&loaded, "make_b", vm.execution_options())
        .unwrap()
        .return_value;
    assert_eq!(
        vm.memory
            .scope(&vm.resources, |cx, _| wrong
                .get(cx)
                .map(|value| value.kind()))
            .unwrap(),
        Some(ObjectKind::Struct)
    );
    let error = vm
        .invoke_interface_method(&boxed, &method, &[wrong])
        .unwrap_err();
    assert!(
        matches!(error, ref error if error.to_string() == "interface method argument does not match its linked signature")
    );
}

#[test]
fn linked_interface_instruction_executes_and_rejects_invalid_slots() {
    let module = interface_instruction_module();
    verify_module(module.records(), module.definitions(), &Default::default()).unwrap();

    let (definitions, mut invalid) = module.clone().into_parts();
    invalid.functions[0].instructions[1] = BytecodeInstruction::MakeInterface {
        dst: Slot::new(1),
        value: Slot::new(0),
        target: BytecodeConstruction::Declared {
            module: ModuleRef::new(0),
            implementation: InterfaceTableRef::new(1),
            arguments: vec![],
            operations: vec![],
            constructions: vec![],
        },
    };
    assert!(verify_module(&invalid, &definitions, &Default::default()).is_err());
    invalid.functions[0].instructions[1] = BytecodeInstruction::MakeInterface {
        dst: Slot::new(1),
        value: Slot::new(1),
        target: BytecodeConstruction::Declared {
            module: ModuleRef::new(0),
            implementation: InterfaceTableRef::new(0),
            arguments: vec![],
            operations: vec![],
            constructions: vec![],
        },
    };
    assert!(verify_module(&invalid, &definitions, &Default::default()).is_err());

    let context = DefinitionContext::new().unwrap();
    let module = module
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let program = interface_program(&context, vec![module], ModuleRef::new(0));
    let decoded = round_trip_bytecode(
        DefinitionMetadata::checked(context.snapshot(), program, &Default::default()).unwrap(),
    );
    let mut runtime = test_runtime(Default::default());
    let loaded = load_bytecode(&mut runtime, "interface-instruction", decoded).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let value = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .unwrap()
        .return_value;
    assert_eq!(
        vm.interface_description(&value).unwrap().concrete_type,
        Ty::Builtin(BuiltinType::I32)
    );
}

#[test]
fn interface_instruction_uses_a_reachable_dependency_table() {
    let context = DefinitionContext::new().unwrap();
    let dependency = interface_instruction_module()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let mut consumer = verified_module(
        vec![test_function(
            0,
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::MakeInterface {
                    dst: Slot::new(1),
                    value: Slot::new(0),
                    target: BytecodeConstruction::Declared {
                        module: ModuleRef::new(0),
                        implementation: InterfaceTableRef::new(0),
                        arguments: vec![],
                        operations: vec![],
                        constructions: vec![],
                    },
                },
                BytecodeInstruction::Return(Some(Slot::new(1))),
            ],
            ValueType::HeapObject,
            vec![ValueType::I32, ValueType::HeapObject],
        )],
        vec![ConstantOperand::I32(11)],
    );
    consumer.identity = ModuleIdentity::single_file("interface-consumer.kgr");
    consumer.dependencies = vec![ModuleRef::new(0)];
    let result = dependency.functions[0]
        .metadata
        .semantic
        .result
        .clone()
        .unwrap();
    consumer.functions[0]
        .metadata
        .semantic
        .slots
        .insert(0, Ty::Builtin(BuiltinType::I32));
    consumer.functions[0].metadata.semantic.result = Some(result.clone());
    consumer.functions[0]
        .metadata
        .semantic
        .slots
        .insert(1, result);
    let program = interface_program(&context, vec![dependency, consumer], ModuleRef::new(1));
    let definitions = identity::collect(
        &program.modules.iter().collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_program(&program, &definitions, &Default::default()).unwrap();
    let mut detached = program.clone();
    detached.modules[1].dependencies.clear();
    assert!(verify_program(&detached, &definitions, &Default::default()).is_err());

    let decoded = round_trip_bytecode(
        DefinitionMetadata::checked(definitions, program, &Default::default()).unwrap(),
    );

    let mut runtime = test_runtime(Default::default());
    let loaded = load_bytecode(&mut runtime, "interface-consumer", decoded).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let dependency_key = loaded.member(ModuleRef::new(0)).unwrap().key();
    let mut vm = runtime;
    let value = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .unwrap()
        .return_value;
    assert_eq!(
        vm.interface_description(&value).unwrap().concrete_type,
        Ty::Builtin(BuiltinType::I32)
    );
    vm.collect_garbage().unwrap();
    assert!(vm.modules().loaded(dependency_key).is_some());
}

#[test]
fn shared_calls_retain_the_callers_nominal_layout_generation() {
    let source = include_str!("../../../tests/fixtures/execution/interface-generic-provenance.kgr");
    check_type_provenance_installations(source, 1);
}

fn check_type_provenance_installations(source: &str, item_constructions: usize) {
    let mut runtime = test_runtime(Default::default());
    let old = load_bytecode(
        &mut runtime,
        "type-provenance",
        compile_test_bytecode(source),
    )
    .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let interface = runtime
        .make_interface(&old, 0, &RootedValue::from(7i32))
        .unwrap();
    let helper = "fn helper() -> i32 {";
    assert_eq!(source.matches(helper).count(), 1);
    let body_start = source.find(helper).unwrap() + helper.len();
    let body_end = body_start + source[body_start..].find('}').unwrap();
    assert_eq!(source[body_start..body_end].trim(), "42");
    let value_start = body_start + source[body_start..body_end].find("42").unwrap();
    let mut second_source = source.to_owned();
    second_source.replace_range(value_start..value_start + 2, "43");
    for (from, to, count) in [
        ("val value: i32", "val value: i32, val extra: bool", 1),
        (
            "Item { value: 1 }",
            "Item { value: 1, extra: true }",
            item_constructions,
        ),
    ] {
        assert_eq!(second_source.matches(from).count(), count, "{from}");
        second_source = second_source.replace(from, to);
    }
    // Frozen declarations reject schema changes before publication.
    let incompatible_source = second_source;
    assert!(
        runtime
            .stage_reload_program(
                &_old_program,
                None,
                compile_test_bytecode(&incompatible_source)
            )
            .is_err()
    );
    let mut second_source = source.to_owned();
    second_source.replace_range(value_start..value_start + 2, "43");
    let candidate = runtime
        .stage_reload_program(&_old_program, None, compile_test_bytecode(&second_source))
        .unwrap();
    let second = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let foreign_program = load_bytecode(
        &mut runtime,
        "foreign-layout",
        compile_test_bytecode(&incompatible_source),
    )
    .unwrap();
    let foreign = runtime.select_program(&foreign_program).unwrap();
    {
        let item_layout = |loaded: &ExecutableModule| {
            let index = loaded
                .bytecode
                .structures
                .iter()
                .position(|layout| {
                    loaded
                        .definitions()
                        .resolve(layout.declaration)
                        .unwrap()
                        .segments()
                        .last()
                        .unwrap()
                        .name
                        == "Item"
                })
                .unwrap();
            loaded.struct_layout(StructId::new(index)).unwrap()
        };
        let old_item = runtime
            .alloc_struct(item_layout(&old), &[RootedValue::from(1i32)])
            .unwrap();
        let current_layout = item_layout(&second);
        let item_type = Ty::Struct(NominalTy {
            declaration: current_layout.layout().declaration,
            arguments: vec![],
            associated_types: Default::default(),
        });
        let current_item = runtime
            .alloc_struct(current_layout, &[RootedValue::from(1i32)])
            .unwrap();
        let trait_type = old
            .bytecode
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicItem::InterfaceTable(table) => match &table.trait_type {
                    Ty::Trait(ty) => Some(ty),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        let arguments = runtime
            .resolve_type_arguments(&second, &[item_type])
            .unwrap();
        let method = runtime
            .resolve_interface_method_slot(&interface, trait_type, 0, &arguments)
            .unwrap();
        runtime
            .validate_interface_method_arguments(&method, &[RootedValue::from(7i32), current_item])
            .unwrap();
        runtime
            .validate_interface_method_arguments(&method, &[RootedValue::from(7i32), old_item])
            .unwrap();
        let foreign_item = runtime
            .alloc_struct(
                item_layout(&foreign),
                &[RootedValue::from(1i32), RootedValue::from(true)],
            )
            .unwrap();
        assert!(
            runtime
                .validate_interface_method_arguments(
                    &method,
                    &[RootedValue::from(7i32), foreign_item]
                )
                .is_err()
        );
        let foreign_arguments = runtime
            .resolve_type_arguments(&foreign, slice::from_ref(arguments[0].ty()))
            .unwrap();
        assert!(
            runtime
                .resolve_interface_method_slot(&interface, trait_type, 0, &foreign_arguments)
                .is_err()
        );
    }
    let make = second
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "make")
        .unwrap()
        .id;
    let closure = {
        let mut conversion = ConversionContext::new(&mut runtime, &second).unwrap();
        let args = conversion
            .root_arguments(slice::from_ref(&interface))
            .unwrap();
        Executor::new(conversion.runtime_mut(), &second, make, args)
            .unwrap()
            .run(|runtime, stack, value| stack.retain_result(runtime, value))
            .unwrap()
    };
    let third_source = second_source.replace("43", "44");
    let candidate = runtime
        .stage_reload_program(&_old_program, None, compile_test_bytecode(&third_source))
        .unwrap();
    let third = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(second.key()).is_some());
    let call = third
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "use_closure")
        .unwrap()
        .id;
    {
        let mut conversion = ConversionContext::new(&mut runtime, &third).unwrap();
        let args = conversion
            .root_arguments(slice::from_ref(&closure))
            .unwrap();
        let result = Executor::new(conversion.runtime_mut(), &third, call, args)
            .unwrap()
            .run(|runtime, stack, value| stack.retain_result(runtime, value))
            .unwrap();
        assert_eq!(result.as_i32(), Some(44));
    }
    drop((closure, interface));
    // Linked construction descriptors retain their supplier until their owning
    // publication is superseded; remove that cache owner before checking release.
    let candidate = runtime
        .stage_reload_program(&_old_program, None, compile_test_bytecode(&third_source))
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old.key()).is_none());
    assert!(runtime.modules().loaded(second.key()).is_none());
    assert!(runtime.validate_executable_module(&second).is_err());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn shared_aggregate_fields_retain_the_callers_nominal_generation() {
    check_type_provenance_installations(
        include_str!("../../../tests/fixtures/execution/interface-aggregate-provenance.kgr"),
        1,
    );
}

#[test]
fn shared_closure_signatures_distinguish_nominal_generations() {
    let source = include_str!("../../../tests/fixtures/execution/interface-closure-signature.kgr");
    let mut runtime = test_runtime(Default::default());
    let old = load_bytecode(&mut runtime, "closure-scope", compile_test_bytecode(source)).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let receiver = runtime
        .make_interface(&old, 0, &RootedValue::from(7i32))
        .unwrap();
    let next_source = source
        .replace("val value: i32", "val value: i32, val extra: bool")
        .replace("value: 1 }", "value: 1, extra: true }");
    assert!(
        runtime
            .stage_reload_program(&_old_program, None, compile_test_bytecode(&next_source))
            .is_err()
    );
    let current_program = load_bytecode(
        &mut runtime,
        "closure-scope-current",
        compile_test_bytecode(&next_source),
    )
    .unwrap();
    let current = runtime.select_program(&current_program).unwrap();
    let current_receiver = runtime
        .make_interface(&current, 0, &RootedValue::from(7i32))
        .unwrap();
    let make = current
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "make")
        .unwrap()
        .id;
    let closure = {
        let mut conversion = ConversionContext::new(&mut runtime, &current).unwrap();
        let args = conversion
            .root_arguments(slice::from_ref(&current_receiver))
            .unwrap();
        Executor::new(conversion.runtime_mut(), &current, make, args)
            .unwrap()
            .run(|runtime, stack, value| stack.retain_result(runtime, value))
            .unwrap()
    };
    let item = current
        .bytecode
        .structures
        .iter()
        .find(|layout| {
            current
                .definitions()
                .resolve(layout.declaration)
                .unwrap()
                .segments()
                .last()
                .unwrap()
                .name
                == "Item"
        })
        .unwrap();
    let ty = Ty::Struct(NominalTy {
        declaration: item.declaration,
        arguments: vec![],
        associated_types: Default::default(),
    });
    let interface = old
        .bytecode
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table) => match &table.trait_type {
                Ty::Trait(ty) => Some(ty),
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    for (owner, receiver, valid) in [
        (&old, &receiver, false),
        (&current, &current_receiver, true),
    ] {
        let arguments = runtime
            .resolve_type_arguments(owner, slice::from_ref(&ty))
            .unwrap();
        let method = runtime
            .resolve_interface_method_slot(receiver, interface, 0, &arguments)
            .unwrap();
        assert_eq!(
            runtime
                .validate_interface_method_result(&method, &closure)
                .is_ok(),
            valid
        );
    }
    let objects = runtime.gc_stats().objects;
    drop((receiver, current_receiver, closure));
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc_stats().objects < objects);
}

#[test]
fn retained_interface_calls_cannot_escape_candidate_initialization() {
    let source = include_str!("../../../tests/fixtures/execution/interface-methods.kgr");

    let (mut vm, old) = load_test_module(source);

    let _old_program = old;

    let old = vm.select_program(&_old_program).unwrap();

    let old_value: Interface = vm.execute_module_typed(&old, "make", (100,)).unwrap();

    let old_member = old_value.member(&mut vm, "get").unwrap();
    let old_get = vm.bind_interface_method::<(), i32>(&old_member).unwrap();

    let candidate = vm
        .stage_reload_program(
            &_old_program,
            Some("functions".into()),
            compile_test_bytecode(source),
        )
        .unwrap();

    {
        let mut _session = vm.begin_candidate_initialization(&candidate).unwrap();

        let candidate_value: Interface = _session
            .runtime_mut()
            .execute_module_typed(candidate.module(), "make", (42,))
            .unwrap();

        let member = candidate_value
            .member(_session.runtime_mut(), "get")
            .unwrap();
        let get = _session
            .runtime_mut()
            .bind_interface_method::<(), i32>(&member)
            .unwrap();

        let mut cx = _session.runtime_mut().context(candidate.module()).unwrap();

        assert!(old_value.call(&mut cx, &old_get, ()).is_err());

        assert_eq!(candidate_value.call(&mut cx, &get, ()).unwrap(), 42);
    }

    assert_eq!(
        old_value
            .call(&mut vm.context(&old).unwrap(), &old_get, ())
            .unwrap(),
        100
    );
}
