use super::*;

#[test]
fn concrete_interface_object_resolves_a_linked_method_slot() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn read<T: Tag>(x: T) -> i32 { x.tag() } fn main() -> i32 { read(7) }",
    );
    let table = &loaded.bytecode.interface_tables[0];
    let method = table.methods[0].method.clone();
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let resolved = runtime.resolve_interface_method(&boxed, &method).unwrap();
    assert_eq!(resolved.receiver(), &Value::I32(7));
    assert_eq!(resolved.implementation().key(), loaded.key());
    assert_eq!(resolved.function(), table.methods[0].function);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&boxed));
    assert!(
        runtime
            .resolve_interface_method(&Value::I32(7), &method)
            .is_err()
    );
    assert!(
        runtime
            .validate_interface_method_result(&resolved, &Value::Bool(true))
            .is_err()
    );

    let mut vm = Vm::new(runtime);
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
    use kagari_abi::types::AbiType;
    use kagari_abi::types::PublicAbiItem;
    let (runtime, loaded) = load_test_module(
        "trait Pair { fn first(self) -> i32; fn second(self) -> i32; } impl Pair for i32 { fn second(self) -> i32 { 2 } fn first(self) -> i32 { 1 } } fn main() -> i32 { 0 }",
    );
    let table = loaded
        .bytecode
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .unwrap();
    let AbiType::Trait(interface) = &table.trait_type else {
        panic!("expected trait interface")
    };
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let first = runtime
        .resolve_interface_method_slot(&boxed, interface, 0)
        .unwrap();
    let second = runtime
        .resolve_interface_method_slot(&boxed, interface, 1)
        .unwrap();
    let method = |name| {
        loaded.bytecode.interface_tables[0]
            .methods
            .iter()
            .find(|slot| slot.method.path.last().unwrap().name == name)
            .unwrap()
            .method
            .clone()
    };
    assert_eq!(
        first.function(),
        runtime
            .resolve_interface_method(&boxed, &method("first"))
            .unwrap()
            .function()
    );
    assert_eq!(
        second.function(),
        runtime
            .resolve_interface_method(&boxed, &method("second"))
            .unwrap()
            .function()
    );
    assert_ne!(first.function(), second.function());
    assert!(
        runtime
            .resolve_interface_method_slot(&boxed, interface, 2)
            .is_err()
    );
    let mut wrong = interface.clone();
    wrong.declaration.path.last_mut().unwrap().name = "Other".into();
    assert!(
        runtime
            .resolve_interface_method_slot(&boxed, &wrong, 0)
            .is_err()
    );
}

#[test]
fn source_call_boxes_a_concrete_argument_for_an_interface_parameter() {
    let (runtime, loaded) = load_test_module(
        "trait Tag {} impl Tag for i32 {} fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { accept(7) }",
    );
    assert!(
        loaded
            .bytecode
            .functions
            .iter()
            .flat_map(|function| &function.instructions)
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeInterface { .. }))
    );
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn source_call_boxes_an_interface_with_methods() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { accept(7) }",
    );
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn source_interface_method_call_dispatches_through_the_linked_slot() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn accept(value: Tag) -> i32 { value.tag() } fn main() -> i32 { accept(7) }",
    );
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(8)
    );
}

#[test]
fn source_return_and_local_bindings_keep_the_boxed_interface_value() {
    let (runtime, loaded) = load_test_module(
        "trait Tag {} impl Tag for i32 {} fn make() -> Tag { 7 } fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { val value: Tag = make(); accept(value) }",
    );
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn interface_method_keeps_its_implementation_across_reload() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn read<T: Tag>(x: T) -> i32 { x.tag() } fn main() -> i32 { read(7) }";
    let first = compile_test_bytecode(source);
    let second = compile_test_bytecode(&source.replace("self + 1", "self + 2"));
    let mut runtime = Runtime::default();
    let program = |module| kagari_bytecode::BytecodeProgram {
        root: kagari_bytecode::ModuleRef::new(0),
        modules: vec![module],
    };
    let old = runtime
        .load_program("interface-reload", program(first))
        .unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method.clone();
    let old_value = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let old_root = runtime.root_value(old_value.clone()).unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "interface-reload", program(second))
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
    let new_value = runtime.make_interface(&new, 0, Value::I32(7)).unwrap();
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.invoke_interface_method(&old_value, &method, &[])
            .unwrap(),
        Value::I32(8)
    );
    assert_eq!(
        vm.invoke_interface_method(&new_value, &method, &[])
            .unwrap(),
        Value::I32(9)
    );
    drop(old_root);
    vm.runtime().collect_garbage().unwrap();
    assert!(!vm.runtime().gc().validate_value(&old_value));
}

#[test]
fn interface_frame_descendants_follow_the_receivers_pinned_program() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { helper() + self } } fn helper() -> i32 { 1 } fn main() -> i32 { 0 }";
    let old_code = compile_test_bytecode(source);
    let new_code = compile_test_bytecode(
        &source.replace("fn helper() -> i32 { 1 }", "fn helper() -> i32 { 2 }"),
    );
    let program = |module| kagari_bytecode::BytecodeProgram {
        root: kagari_bytecode::ModuleRef::new(0),
        modules: vec![module],
    };
    let mut runtime = Runtime::default();
    let old = runtime
        .load_program("interface-frames", program(old_code))
        .unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method.clone();
    let boxed = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let _root = runtime.root_value(boxed.clone()).unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "interface-frames", program(new_code))
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
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
    let stack = runtime.enter_execution_stack(&new).unwrap();
    stack.push(new.slot(), entry, &[], None).unwrap();
    let wrong = runtime.resolve_interface_method(&boxed, &method).unwrap();
    assert!(
        stack
            .push_interface_method(&runtime, wrong, &[Value::Bool(true)], None)
            .is_err()
    );
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack
        .push_interface_method(&runtime, resolved, &[Value::I32(7)], None)
        .unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), old.key());
    stack.push(old.slot(), helper, &[], None).unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), old.key());
    stack.pop().unwrap();
    stack.pop().unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), new.key());
    stack.pop().unwrap();
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn source_interface_dispatch_keeps_old_method_and_descendant_after_reload() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { helper() + self } } fn helper() -> i32 { 1 } fn read(value: Tag) -> i32 { value.tag() } fn main() -> i32 { read(7) }";
    let old_code = compile_test_bytecode(source);
    let new_code = compile_test_bytecode(
        &source.replace("fn helper() -> i32 { 1 }", "fn helper() -> i32 { 2 }"),
    );
    let program = |module| kagari_bytecode::BytecodeProgram {
        root: kagari_bytecode::ModuleRef::new(0),
        modules: vec![module],
    };
    let mut runtime = Runtime::default();
    let old = runtime
        .load_program("interface-dispatch-reload", program(old_code))
        .unwrap();
    let old_value = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let old_root = runtime.root_value(old_value.clone()).unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "interface-dispatch-reload", program(new_code))
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
    let new_value = runtime.make_interface(&new, 0, Value::I32(7)).unwrap();
    let read = new
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "read")
        .unwrap()
        .id;
    let mut old_call = crate::executor::Executor::new(&runtime, &new, read, &[old_value]).unwrap();
    assert_eq!(old_call.run().unwrap(), Value::I32(8));
    drop(old_call);
    let mut new_call = crate::executor::Executor::new(&runtime, &new, read, &[new_value]).unwrap();
    assert_eq!(new_call.run().unwrap(), Value::I32(9));
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(runtime.modules().collect_unreachable_epochs().is_empty());
    drop(new_call);
    drop(old_root);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.modules().collect_unreachable_epochs(),
        vec![old.key()]
    );
}

#[test]
fn trapped_interface_frame_releases_its_roots_and_call_budget() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self / 0 } } fn main() -> i32 { 42 }",
    );
    let method = loaded.bytecode.interface_tables[0].methods[0]
        .method
        .clone();
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let mut vm = Vm::new(runtime);
    assert!(vm.invoke_interface_method(&boxed, &method, &[]).is_err());
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}

#[test]
fn interface_method_rejects_wrong_nominal_argument_before_execution() {
    let (runtime, loaded) = load_test_module(
        "struct A { val n: i32 } struct B { val n: i32 } trait Tag { fn read(self, x: A) -> i32; } impl Tag for i32 { fn read(self, x: A) -> i32 { x.n } } fn run<T: Tag>(x: T, a: A) -> i32 { x.read(a) } fn main() -> i32 { run(7, A { n: 1 }) } fn make_a() -> A { A { n: 5 } } fn make_b() -> B { B { n: 9 } }",
    );
    let method = loaded.bytecode.interface_tables[0].methods[0]
        .method
        .clone();
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let mut vm = Vm::new(runtime);
    let right = vm.execute(&loaded, "make_a").unwrap().return_value;
    assert_eq!(
        vm.invoke_interface_method(&boxed, &method, &[right])
            .unwrap(),
        Value::I32(5)
    );
    let wrong = vm.execute(&loaded, "make_b").unwrap().return_value;
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
    use kagari_bytecode::ArtifactBuildOptions;
    use kagari_bytecode::ArtifactCompatibility;
    use kagari_bytecode::BytecodeProgram;
    use kagari_bytecode::InterfaceTableRef;
    use kagari_bytecode::KbcArtifact;
    use kagari_bytecode::ModuleRef;
    use kagari_bytecode::verify_module;
    let module = interface_instruction_module();
    verify_module(&module).unwrap();

    let mut invalid = module.clone();
    invalid.functions[0].instructions[1] = BytecodeInstruction::MakeInterface {
        dst: Register::new(1),
        value: Register::new(0),
        module: ModuleRef::new(0),
        implementation: InterfaceTableRef::new(1),
    };
    assert!(verify_module(&invalid).is_err());
    invalid.functions[0].instructions[1] = BytecodeInstruction::MakeInterface {
        dst: Register::new(1),
        value: Register::new(1),
        module: ModuleRef::new(0),
        implementation: InterfaceTableRef::new(0),
    };
    assert!(verify_module(&invalid).is_err());

    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program("interface-instruction", decoded.program)
        .unwrap();
    let mut vm = Vm::new(runtime);
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    assert!(matches!(value, Value::Interface(_)));
    assert!(vm.runtime().gc().validate_value(&value));
}

#[test]
fn interface_instruction_uses_a_reachable_dependency_table() {
    use kagari_bytecode::ArtifactBuildOptions;
    use kagari_bytecode::ArtifactCompatibility;
    use kagari_bytecode::BytecodeProgram;
    use kagari_bytecode::InterfaceTableRef;
    use kagari_bytecode::KbcArtifact;
    use kagari_bytecode::ModuleRef;
    use kagari_bytecode::verify_program;
    use kagari_common::identity::ModuleIdentity;

    let dependency = interface_instruction_module();
    let mut consumer = verified_module(vec![test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Register::new(0),
                constant: ConstantOperand::I32(11),
            },
            BytecodeInstruction::MakeInterface {
                dst: Register::new(1),
                value: Register::new(0),
                module: ModuleRef::new(0),
                implementation: InterfaceTableRef::new(0),
            },
            BytecodeInstruction::Return(Some(Register::new(1))),
        ],
        ValueType::HeapObject,
        vec![ValueType::I32, ValueType::HeapObject],
    )]);
    consumer.identity = ModuleIdentity::single_file("interface-consumer.kgr");
    consumer.dependencies = vec![ModuleRef::new(0)];
    let program = BytecodeProgram {
        root: ModuleRef::new(1),
        modules: vec![dependency, consumer],
    };
    verify_program(&program).unwrap();
    let mut detached = program.clone();
    detached.modules[1].dependencies.clear();
    assert!(verify_program(&detached).is_err());

    let artifact = KbcArtifact::from_program(program, ArtifactBuildOptions::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();

    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program("interface-consumer", decoded.program)
        .unwrap();
    let dependency_key = loaded.member(ModuleRef::new(0)).unwrap().key();
    let mut vm = Vm::new(runtime);
    let value = vm.execute(&loaded, "main").unwrap().return_value;
    assert!(matches!(value, Value::Interface(_)));
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(dependency_key)
            .runtime_values,
        1
    );
}
