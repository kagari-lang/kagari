mod fixtures;

use self::fixtures::closure_constraint_provider;
use super::*;
use crate::{
    executor::Executor,
    tests::{
        common::{round_trip_bytecode, standard_runtime},
        native_boundary::compile_without_foundation,
    },
    vm::fixtures::FixtureExecution,
};
use kagari_bytecode::instruction::ConstructionTarget as BytecodeConstruction;
use kagari_bytecode::{
    instruction::{ConstantId, StructId},
    program::{identity, verify_program},
    verifier::{BytecodeVerificationError, verify_module},
};

use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    frame::{
        driver::{ExecutionAction, ReturnAction},
        transfer::ReturnValue,
    },
    module::{ExecutableModule, entry::ProgramEntry},
};
use kagari_types::ty::{NominalTy, Ty};
use std::slice;

#[test]
fn concrete_interface_object_resolves_a_linked_method_slot() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn read<T: Tag>(x: T) -> i32 { x.tag() } fn main() -> i32 { read(7) }",
    );
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let table = &loaded.bytecode.interface_tables[0];
    let method = table.methods[0].method;
    let boxed = unsafe { runtime.make_interface_transport(&loaded, 0, Value::I32(7)) }.unwrap();
    let resolved = unsafe { runtime.resolve_interface_method_transport(&boxed, &method) }.unwrap();
    assert_eq!(unsafe { resolved.receiver_transport() }, &Value::I32(7));
    assert_eq!(
        resolved.implementation(&runtime).unwrap().key(),
        loaded.key()
    );
    assert_eq!(resolved.target(&runtime).unwrap(), table.methods[0].target);
    let foreign = Runtime::default();
    assert!(resolved.implementation(&foreign).is_err());
    assert!(resolved.target(&foreign).is_err());
    assert!(resolved.parameter_types(&foreign).is_err());
    assert!(resolved.return_type(&foreign).is_err());
    runtime.collect_garbage().unwrap();
    assert!(unsafe { runtime.gc().validate_value_transport(&boxed) });
    assert!(
        unsafe { runtime.resolve_interface_method_transport(&Value::I32(7), &method) }.is_err()
    );
    assert!(
        unsafe {
            runtime.validate_interface_method_result_transport(&resolved, &Value::Bool(true))
        }
        .is_err()
    );

    let vm = Vm::new(runtime);
    assert_eq!(
        vm.invoke_interface_method(&boxed, &method, &[]).unwrap(),
        Value::I32(8)
    );
    assert!(
        vm.invoke_interface_method(&boxed, &method, &[Value::Bool(true)])
            .is_err()
    );
}

#[test]
fn interface_method_slots_follow_trait_order_even_when_impl_order_differs() {
    let (runtime, loaded) = load_test_module(
        "trait Other {} trait Pair { fn first(self) -> i32; fn second(self) -> i32; } impl Pair for i32 { fn second(self) -> i32 { 2 } fn first(self) -> i32 { 1 } } fn main() -> i32 { 0 }",
    );
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
    let boxed = unsafe { runtime.make_interface_transport(&loaded, 0, Value::I32(7)) }.unwrap();
    let first =
        unsafe { runtime.resolve_interface_method_slot_transport(&boxed, interface, 0, &[]) }
            .unwrap();
    let second =
        unsafe { runtime.resolve_interface_method_slot_transport(&boxed, interface, 1, &[]) }
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
        first.target(&runtime).unwrap(),
        unsafe { runtime.resolve_interface_method_transport(&boxed, &method("first")) }
            .unwrap()
            .target(&runtime)
            .unwrap()
    );
    assert_eq!(
        second.target(&runtime).unwrap(),
        unsafe { runtime.resolve_interface_method_transport(&boxed, &method("second")) }
            .unwrap()
            .target(&runtime)
            .unwrap()
    );
    assert_ne!(
        first.target(&runtime).unwrap(),
        second.target(&runtime).unwrap()
    );
    assert!(
        unsafe { runtime.resolve_interface_method_slot_transport(&boxed, interface, 2, &[]) }
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
        unsafe { runtime.resolve_interface_method_slot_transport(&boxed, &wrong, 0, &[]) }.is_err()
    );
}

#[test]
fn source_call_boxes_a_concrete_argument_for_an_interface_parameter() {
    let (runtime, loaded) = load_test_module(
        "trait Tag {} impl Tag for i32 {} fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { accept(7) }",
    );
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
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn source_call_boxes_an_interface_with_methods() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { accept(7) }",
    );
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn source_interface_method_call_dispatches_through_the_linked_slot() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn accept(value: Tag) -> i32 { value.tag() } fn main() -> i32 { accept(7) }",
    );
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(8)
    );
}

#[test]
fn static_bound_calls_preserve_explicit_self_typed_arguments_and_checked_signatures() {
    let source = r#"
        trait Factory {
            fn make() -> Self;
            fn shift(value: Self, by: i32) -> Self;
            fn read(self) -> i32;
        }
        struct Item { val value: i32 }
        impl Factory for Item {
            fn make() -> Self { Item { value: 20 } }
            fn shift(value: Self, by: i32) -> Self { Item { value: value.value + by } }
            fn read(self) -> i32 { self.value }
        }
        trait Relay {
            fn relay<T: Factory>(self, value: T) -> i32 {
                T::shift(value, 2).read() + T::make().read()
            }
        }
        impl Relay for i32 {}
        fn main() -> i32 { val relay: Relay = 0; relay.relay(Item { value: 20 }) }
    "#;
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
    let mut runtime = standard_runtime(config);
    let loaded = runtime.load_program("static-bound", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn source_return_and_local_bindings_keep_the_boxed_interface_value() {
    let (runtime, loaded) = load_test_module(
        "trait Tag {} impl Tag for i32 {} fn make() -> Tag { 7 } fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { val value: Tag = make(); accept(value) }",
    );
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn interface_method_selects_current_implementation_after_reload() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn read<T: Tag>(x: T) -> i32 { x.tag() } fn main() -> i32 { read(7) }";
    let first = compile_test_bytecode(source);
    let second = compile_test_bytecode(&source.replace("self + 1", "self + 2"));
    let mut runtime = standard_runtime(Default::default());
    let old = runtime.load_program("interface-reload", first).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method;
    let old_value = unsafe { runtime.make_interface_transport(&old, 0, Value::I32(7)) }.unwrap();
    let old_root = unsafe { runtime.gc().root_value_transport(old_value) }.unwrap();
    let candidate = runtime
        .stage_reload_program(&_old_program, Some("interface-reload".into()), second)
        .unwrap();
    let new = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let _new_program = _old_program.clone();
    let new_value = unsafe { runtime.make_interface_transport(&new, 0, Value::I32(7)) }.unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.invoke_interface_method(&old_value, &method, &[])
            .unwrap(),
        Value::I32(9)
    );
    assert_eq!(
        vm.invoke_interface_method(&new_value, &method, &[])
            .unwrap(),
        Value::I32(9)
    );
    drop(old_root);
    vm.runtime().collect_garbage().unwrap();
    assert!(!unsafe { vm.runtime().gc().validate_value_transport(&old_value) });
}

#[test]
fn interface_frames_and_descendants_select_current_code() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { helper() + self } } fn helper() -> i32 { 1 } fn main() -> i32 { 0 }";
    let old_code = compile_test_bytecode(source);
    let new_code = compile_test_bytecode(
        &source.replace("fn helper() -> i32 { 1 }", "fn helper() -> i32 { 2 }"),
    );
    let mut runtime = standard_runtime(Default::default());
    let old = runtime.load_program("interface-frames", old_code).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method;
    let boxed = unsafe { runtime.make_interface_transport(&old, 0, Value::I32(7)) }.unwrap();
    let receiver_root = unsafe { runtime.gc().root_value_transport(boxed) }.unwrap();
    let candidate = runtime
        .stage_reload_program(&_old_program, Some("interface-frames".into()), new_code)
        .unwrap();
    let new = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let _new_program = _old_program.clone();
    let resolved = unsafe { runtime.resolve_interface_method_transport(&boxed, &method) }.unwrap();
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
    let stack = runtime.enter_execution_stack(&new).unwrap();
    unsafe { stack.push_transport(&runtime, new.slot(), entry, &[], None) }.unwrap();
    let wrong = unsafe { runtime.resolve_interface_method_transport(&boxed, &method) }.unwrap();
    assert!(
        unsafe {
            stack.push_interface_method_transport(&runtime, wrong, &[Value::Bool(true)], None)
        }
        .is_err()
    );
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    unsafe { stack.push_interface_method_transport(&runtime, resolved, &[Value::I32(7)], None) }
        .unwrap();
    // The callee must retain selection metadata after its host lease and the
    // receiver's external root are gone, even while a newer program is current.
    drop(receiver_root);
    runtime.collect_garbage().unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    unsafe { stack.push_transport(&runtime, new.slot(), helper, &[], None) }.unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack.pop().unwrap();
    assert_eq!(
        unsafe { stack.finish_return_transport(&runtime, ReturnValue::general(Value::I32(8))) }
            .unwrap(),
        ReturnAction::Continue(ExecutionAction::Script)
    );
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack.pop().unwrap();
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn source_interface_dispatch_selects_current_method_and_descendant_after_reload() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { helper() + self } } fn helper() -> i32 { 1 } fn read(value: Tag) -> i32 { value.tag() } fn main() -> i32 { read(7) }";
    let old_code = compile_test_bytecode(source);
    let new_code = compile_test_bytecode(
        &source.replace("fn helper() -> i32 { 1 }", "fn helper() -> i32 { 2 }"),
    );
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("interface-dispatch-reload", old_code)
        .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let old_value = unsafe { runtime.make_interface_transport(&old, 0, Value::I32(7)) }.unwrap();
    let old_root = unsafe { runtime.gc().root_value_transport(old_value) }.unwrap();
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
    let new_value = unsafe { runtime.make_interface_transport(&new, 0, Value::I32(7)) }.unwrap();
    let read = new
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "read")
        .unwrap()
        .id;
    let mut old_call = Executor::new(&runtime, &new, read, &[old_value]).unwrap();
    assert_eq!(old_call.run().unwrap(), Value::I32(9));
    drop(old_call);
    let mut new_call = Executor::new(&runtime, &new, read, &[new_value]).unwrap();
    assert_eq!(new_call.run().unwrap(), Value::I32(9));
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    drop(new_call);
    drop(old_root);
    let expected: std::collections::HashSet<_> = old.members().map(|member| member.key()).collect();
    let reclaimed: std::collections::HashSet<_> = runtime
        .collect_garbage()
        .unwrap()
        .reclaimed_modules
        .into_iter()
        .collect();
    assert_eq!(reclaimed, expected);
}

#[test]
fn generic_interface_and_retained_closure_keep_their_environment_after_reload() {
    let source = include_str!("../../../tests/fixtures/execution/generic-interface-capture.kgr");
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("shared-reload", compile_test_bytecode(source))
        .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let interface = unsafe { runtime.make_interface_transport(&old, 0, Value::I32(7)) }.unwrap();
    let interface_root = unsafe { runtime.gc().root_value_transport(interface) }.unwrap();
    let make = old
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "make")
        .unwrap()
        .id;
    let closure = {
        let mut execution = Executor::new(&runtime, &old, make, &[]).unwrap();
        execution.run().unwrap()
    };
    let closure_root = unsafe { runtime.gc().root_value_transport(closure) }.unwrap();
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
    let first_reclaimed = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(!first_reclaimed.contains(&old.key()));
    for (name, args, expected) in [
        ("read", vec![interface], 43),
        ("use_closure", vec![closure], 43),
        ("main", vec![], 43),
    ] {
        let entry = new
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap()
            .id;
        let mut execution = Executor::new(&runtime, &new, entry, &args).unwrap();
        assert_eq!(execution.run().unwrap(), Value::I32(expected));
    }
    drop(interface_root);
    drop(closure_root);
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
    let retained = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(retained.contains(&old.key()));
}

#[test]
fn trapped_interface_frame_releases_its_roots_and_call_depth() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self / 0 } } fn main() -> i32 { 42 }",
    );
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let method = loaded.bytecode.interface_tables[0].methods[0].method;
    let boxed = unsafe { runtime.make_interface_transport(&loaded, 0, Value::I32(7)) }.unwrap();
    let vm = Vm::new(runtime);
    assert!(vm.invoke_interface_method(&boxed, &method, &[]).is_err());
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn interface_method_rejects_wrong_nominal_argument_before_execution() {
    let (runtime, loaded) = load_test_module(
        "struct A { val n: i32 } struct B { val n: i32 } trait Tag { fn read(self, x: A) -> i32; } impl Tag for i32 { fn read(self, x: A) -> i32 { x.n } } fn run<T: Tag>(x: T, a: A) -> i32 { x.read(a) } fn main() -> i32 { run(7, A { n: 1 }) } pub fn make_a() -> A { A { n: 5 } } pub fn make_b() -> B { B { n: 9 } }",
    );
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let method = loaded.bytecode.interface_tables[0].methods[0].method;
    let boxed = unsafe { runtime.make_interface_transport(&loaded, 0, Value::I32(7)) }.unwrap();
    let vm = Vm::new(runtime);
    let right_root = vm.execute(&loaded, "make_a").unwrap().return_value;
    // SAFETY: The result root protects this checked interface operation.
    let right = unsafe { right_root.value_transport(vm.runtime().gc()) }.unwrap();
    assert_eq!(
        vm.invoke_interface_method(&boxed, &method, &[right])
            .unwrap(),
        Value::I32(5)
    );
    let wrong_root = vm.execute(&loaded, "make_b").unwrap().return_value;
    // SAFETY: The result root protects this checked interface operation.
    let wrong = unsafe { wrong_root.value_transport(vm.runtime().gc()) }.unwrap();
    assert!(matches!(wrong, Value::Struct(_)));
    let error = vm
        .invoke_interface_method(&boxed, &method, &[wrong])
        .unwrap_err();
    assert!(
        matches!(error, VmError::RuntimeError(ref error) if error.message() == "interface method argument does not match its linked signature")
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
    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime
        .load_program("interface-instruction", decoded)
        .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    let value_root = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .unwrap()
        .return_value;
    // SAFETY: The result root protects this checked interface operation.
    let value = unsafe { value_root.value_transport(vm.runtime().gc()) }.unwrap();
    assert!(matches!(value, Value::Interface(_)));
    assert!(unsafe { vm.runtime().gc().validate_value_transport(&value) });
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

    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime.load_program("interface-consumer", decoded).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let dependency_key = loaded.member(ModuleRef::new(0)).unwrap().key();
    let vm = Vm::new(runtime);
    let value_root = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .unwrap()
        .return_value;
    // SAFETY: The result root protects this checked interface operation.
    let value = unsafe { value_root.value_transport(vm.runtime().gc()) }.unwrap();
    assert!(matches!(value, Value::Interface(_)));
    let _root = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(dependency_key).is_some());
}

#[test]
fn a_retained_generic_closure_keeps_constraint_evidence_but_calls_current_code() {
    let source =
        include_str!("../../../tests/fixtures/execution/constraint-generation-capture.kgr");
    fn execute(runtime: &Runtime, module: &ExecutableModule, name: &str, args: &[Value]) -> Value {
        let entry = module
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap()
            .id;
        Executor::new(runtime, module, entry, args)
            .unwrap()
            .run()
            .unwrap()
    }
    // Keep this fixture focused on its registered callable constraint protocol.
    let provider = closure_constraint_provider();
    let compile = |text: &str| compile_without_foundation(text, Some(&provider));
    let mut runtime = Runtime::default();
    provider.install(&mut runtime).unwrap();
    let first = runtime
        .load_program("constraint-reload", compile(source))
        .unwrap();
    let _first_program = first;
    let first = runtime.select_program(&_first_program).unwrap();
    let receiver = execute(&runtime, &first, "source", &[]);
    let receiver_root = unsafe { runtime.gc().root_value_transport(receiver) }.unwrap();
    assert_eq!(source.matches("42").count(), 1);
    let candidate = runtime
        .stage_reload_program(
            &_first_program,
            Some("constraint-reload".into()),
            compile(&source.replace("42", "43")),
        )
        .unwrap();
    let second = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let _second_program = _first_program.clone();
    let closure = execute(&runtime, &second, "make", &[receiver]);
    let closure_root = unsafe { runtime.gc().root_value_transport(closure) }.unwrap();
    drop(receiver_root);
    let candidate = runtime
        .stage_reload_program(
            &_second_program,
            Some("constraint-reload".into()),
            compile(&source.replace("42", "44")),
        )
        .unwrap();
    let third = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let _third_program = _second_program.clone();
    let reclaimed = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(!reclaimed.contains(&first.key()));
    assert!(!reclaimed.contains(&second.key()));
    assert_eq!(
        execute(&runtime, &third, "use_closure", &[closure]),
        Value::I32(44)
    );
    drop(closure_root);
    let reclaimed = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(reclaimed.contains(&first.key()));
    assert!(reclaimed.contains(&second.key()));
}

#[test]
fn shared_calls_retain_the_callers_nominal_layout_generation() {
    let source = r#"
        struct Item { val value: i32 }
        fn keep<T>(value: T) -> T { value }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                || { val held: T = keep(value); helper() }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Item { value: 1 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
    "#;
    check_type_provenance_installations(source, 1);
}

#[test]
fn shared_composite_arguments_keep_distinct_nominal_generations() {
    check_type_provenance_installations(
        r#"
        struct Item { val value: i32 }
        fn keep<T>(value: T) -> T { value }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                val mixed: (Item, Option<T>) = (Item { value: 1 }, Some(value));
                || { val held: (Item, Option<T>) = keep(mixed); helper() }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Item { value: 1 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
    "#,
        2,
    );
}

#[test]
fn shared_closures_keep_type_metadata_and_pinned_construction_dependencies() {
    check_type_provenance_installations(
        r#"
        struct Item { val value: i32 }
        fn keep<T>(value: T) -> T { value }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                || { val empty: Option<T> = None; val held: Option<T> = keep(empty); helper() }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Item { value: 1 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
    "#,
        1,
    );
}

fn check_type_provenance_installations(source: &str, item_constructions: usize) {
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("type-provenance", compile_test_bytecode(source))
        .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let interface = unsafe { runtime.make_interface_transport(&old, 0, Value::I32(7)) }.unwrap();
    let interface_root = unsafe { runtime.gc().root_value_transport(interface) }.unwrap();
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
    let foreign_program = runtime
        .load_program(
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
        let old_item = Value::Struct(
            unsafe { runtime.alloc_struct_transport(item_layout(&old), vec![Value::I32(1)]) }
                .unwrap(),
        );
        let old_root = unsafe { runtime.gc().root_value_transport(old_item) }.unwrap();
        let current_layout = item_layout(&second);
        let item_type = Ty::Struct(NominalTy {
            declaration: current_layout.layout().declaration,
            arguments: vec![],
            associated_types: Default::default(),
        });
        let current_item = Value::Struct(
            unsafe { runtime.alloc_struct_transport(current_layout, vec![Value::I32(1)]) }.unwrap(),
        );
        let current_root = unsafe { runtime.gc().root_value_transport(current_item) }.unwrap();
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
        let method = unsafe {
            runtime.resolve_interface_method_slot_transport(&interface, trait_type, 0, &arguments)
        }
        .unwrap();
        unsafe {
            runtime.validate_interface_method_arguments_transport(
                &method,
                &[Value::I32(7), current_item],
            )
        }
        .unwrap();
        unsafe {
            runtime
                .validate_interface_method_arguments_transport(&method, &[Value::I32(7), old_item])
        }
        .unwrap();
        let foreign_item = Value::Struct(
            unsafe {
                runtime.alloc_struct_transport(
                    item_layout(&foreign),
                    vec![Value::I32(1), Value::Bool(true)],
                )
            }
            .unwrap(),
        );
        assert!(
            unsafe {
                runtime.validate_interface_method_arguments_transport(
                    &method,
                    &[Value::I32(7), foreign_item],
                )
            }
            .is_err()
        );
        let foreign_arguments = runtime
            .resolve_type_arguments(&foreign, slice::from_ref(arguments[0].ty()))
            .unwrap();
        assert!(
            unsafe {
                runtime.resolve_interface_method_slot_transport(
                    &interface,
                    trait_type,
                    0,
                    &foreign_arguments,
                )
            }
            .is_err()
        );
        drop((old_root, current_root));
    }
    let make = second
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "make")
        .unwrap()
        .id;
    let closure = Executor::new(&runtime, &second, make, &[interface])
        .unwrap()
        .run()
        .unwrap();
    let root = unsafe { runtime.gc().root_value_transport(closure) }.unwrap();
    let third_source = second_source.replace("43", "44");
    let candidate = runtime
        .stage_reload_program(&_old_program, None, compile_test_bytecode(&third_source))
        .unwrap();
    let third = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let before_call = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(!before_call.contains(&second.key()));
    let call = third
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "use_closure")
        .unwrap()
        .id;
    assert_eq!(
        Executor::new(&runtime, &third, call, &[closure])
            .unwrap()
            .run()
            .unwrap(),
        Value::I32(44)
    );
    drop(root);
    drop(interface_root);
    // Linked construction descriptors retain their supplier until their owning
    // publication is superseded; remove that cache owner before checking release.
    let candidate = runtime
        .stage_reload_program(&_old_program, None, compile_test_bytecode(&third_source))
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let reclaimed = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(before_call.contains(&old.key()) || reclaimed.contains(&old.key()));
    assert!(reclaimed.contains(&second.key()));
    assert!(runtime.validate_executable_module(&second).is_err());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn shared_aggregate_fields_retain_the_callers_nominal_generation() {
    check_type_provenance_installations(
        r#"
        struct Item { val value: i32 }
        struct Box<T> { var item: T }
        enum Wrapped<T> { Some(T), None }
        fn keep<T>(value: T) -> T { value }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                val boxed: Box<Wrapped<T>> = Box { item: Wrapped::Some(value) };
                boxed.item = Wrapped::Some(value);
                || {
                    val held: Box<Wrapped<T>> = keep(boxed);
                    match held.item { Wrapped::Some(inner) => { val again: T = keep(inner); helper() }, Wrapped::None => 0 }
                }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Item { value: 1 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
    "#,
        1,
    );
}

#[test]
fn shared_native_lists_retain_the_callers_nominal_generation() {
    check_type_provenance_installations(
        r#"use std::collections::{List};

        struct Item { val value: i32 }
        fn keep<T>(value: T) -> T { value }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                val list: List<T> = Vec::from([value]);
                || { val held: T = keep(list[0]); helper() }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Item { value: 1 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
    "#,
        1,
    );
}

#[test]
fn shared_closure_signatures_distinguish_nominal_generations() {
    let source = r#"
        struct Item { val value: i32 }
        trait Capture { fn capture<T>(self, value: T) -> fn() -> T { || value } }
        impl Capture for i32 {}
        fn make(source: Capture) -> fn() -> Item { source.capture(Item { value: 1 }) }
    "#;
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("closure-scope", compile_test_bytecode(source))
        .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let receiver = unsafe { runtime.make_interface_transport(&old, 0, Value::I32(7)) }.unwrap();
    let receiver_root = unsafe { runtime.gc().root_value_transport(receiver) }.unwrap();
    let next_source = source
        .replace("val value: i32", "val value: i32, val extra: bool")
        .replace("value: 1 }", "value: 1, extra: true }");
    assert!(
        runtime
            .stage_reload_program(&_old_program, None, compile_test_bytecode(&next_source))
            .is_err()
    );
    let current_program = runtime
        .load_program("closure-scope-current", compile_test_bytecode(&next_source))
        .unwrap();
    let current = runtime.select_program(&current_program).unwrap();
    let current_receiver =
        unsafe { runtime.make_interface_transport(&current, 0, Value::I32(7)) }.unwrap();
    let current_receiver_root =
        unsafe { runtime.gc().root_value_transport(current_receiver) }.unwrap();
    let make = current
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "make")
        .unwrap()
        .id;
    let closure = Executor::new(&runtime, &current, make, slice::from_ref(&current_receiver))
        .unwrap()
        .run()
        .unwrap();
    let closure_root = unsafe { runtime.gc().root_value_transport(closure) }.unwrap();
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
    for (owner, receiver, valid) in [(&old, receiver, false), (&current, current_receiver, true)] {
        let arguments = runtime
            .resolve_type_arguments(owner, slice::from_ref(&ty))
            .unwrap();
        let method = unsafe {
            runtime.resolve_interface_method_slot_transport(&receiver, interface, 0, &arguments)
        }
        .unwrap();
        assert_eq!(
            unsafe { runtime.validate_interface_method_result_transport(&method, &closure) }
                .is_ok(),
            valid
        );
    }
    drop((receiver_root, current_receiver_root, closure_root));
    runtime.collect_garbage().unwrap();
    assert!(!unsafe { runtime.gc().validate_value_transport(&closure) });
}

#[test]
fn shared_mutable_lists_preserve_mixed_nominal_scopes_and_parent_views() {
    check_type_provenance_installations(
        include_str!("../../../tests/fixtures/execution/mixed-nominal-list-capture.kgr"),
        4,
    );
}

#[test]
fn shared_repeat_arrays_keep_scalar_contracts_in_generic_frames() {
    check_type_provenance_installations(
        r#"use std::collections::{List};

        struct Item { val value: i32 }
        fn keep<T>(value: T) -> T { value }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                val list: [i32] = [42; 2];
                || { val held: i32 = keep(list[1]); helper() + held - 42 }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Item { value: 1 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
    "#,
        1,
    );
}
