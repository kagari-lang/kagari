use super::*;
use crate::{executor::Executor, tests::common::standard_runtime};
use kagari_bytecode::instruction::ConstantId;
use kagari_bytecode::instruction::StructId;
use kagari_contract::types::PublicItem;
use kagari_runtime::{Runtime, frame::transfer::ReturnValue, module::LoadedModule};
use kagari_types::ty::{NominalTy, Ty};
use std::slice;

#[test]
fn concrete_interface_object_resolves_a_linked_method_slot() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn read<T: Tag>(x: T) -> i32 { x.tag() } fn main() -> i32 { read(7) }",
    );
    let table = &loaded.bytecode.interface_tables[0];
    let method = table.methods[0].method;
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let resolved = runtime.resolve_interface_method(&boxed, &method).unwrap();
    assert_eq!(resolved.receiver(), &Value::I32(7));
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
    use {kagari_contract::types::PublicItem, kagari_types::ty::Ty};
    let (runtime, loaded) = load_test_module(
        "trait Other {} trait Pair { fn first(self) -> i32; fn second(self) -> i32; } impl Pair for i32 { fn second(self) -> i32 { 2 } fn first(self) -> i32 { 1 } } fn main() -> i32 { 0 }",
    );
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
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
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
        first.target(&runtime).unwrap(),
        runtime
            .resolve_interface_method(&boxed, &method("first"))
            .unwrap()
            .target(&runtime)
            .unwrap()
    );
    assert_eq!(
        second.target(&runtime).unwrap(),
        runtime
            .resolve_interface_method(&boxed, &method("second"))
            .unwrap()
            .target(&runtime)
            .unwrap()
    );
    assert_ne!(
        first.target(&runtime).unwrap(),
        second.target(&runtime).unwrap()
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
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn source_call_boxes_an_interface_with_methods() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { accept(7) }",
    );
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn source_interface_method_call_dispatches_through_the_linked_slot() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn accept(value: Tag) -> i32 { value.tag() } fn main() -> i32 { accept(7) }",
    );
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(8)
    );
}

#[test]
fn source_return_and_local_bindings_keep_the_boxed_interface_value() {
    let (runtime, loaded) = load_test_module(
        "trait Tag {} impl Tag for i32 {} fn make() -> Tag { 7 } fn accept(value: Tag) -> i32 { 42 } fn main() -> i32 { val value: Tag = make(); accept(value) }",
    );
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn interface_method_keeps_its_implementation_across_reload() {
    let source = "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } } fn read<T: Tag>(x: T) -> i32 { x.tag() } fn main() -> i32 { read(7) }";
    let first = compile_test_bytecode(source);
    let second = compile_test_bytecode(&source.replace("self + 1", "self + 2"));
    let mut runtime = standard_runtime(Default::default());
    let old = runtime.load_program("interface-reload", first).unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method;
    let old_value = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let old_root = runtime.root_value(old_value).unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "interface-reload", second)
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
    let new_value = runtime.make_interface(&new, 0, Value::I32(7)).unwrap();
    let vm = Vm::new(runtime);
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
    let mut runtime = standard_runtime(Default::default());
    let old = runtime.load_program("interface-frames", old_code).unwrap();
    let method = old.bytecode.interface_tables[0].methods[0].method;
    let boxed = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let receiver_root = runtime.root_value(boxed).unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "interface-frames", new_code)
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
    stack.push(&runtime, new.slot(), entry, &[], None).unwrap();
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
    // The callee must retain selection metadata after its host lease and the
    // receiver's external root are gone, even while a newer program is current.
    drop(receiver_root);
    runtime.collect_garbage().unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), old.key());
    stack.push(&runtime, old.slot(), helper, &[], None).unwrap();
    assert_eq!(stack.current().unwrap().loaded().key(), old.key());
    stack.pop().unwrap();
    assert!(
        stack
            .finish_return(&runtime, ReturnValue::general(Value::I32(8)))
            .unwrap()
            .is_none()
    );
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
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("interface-dispatch-reload", old_code)
        .unwrap();
    let old_value = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let old_root = runtime.root_value(old_value).unwrap();
    let candidate = runtime
        .stage_reload_program(&old, "interface-dispatch-reload", new_code)
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
    let source = r#"
        struct Item { val value: i32 }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                || { val held: T = value; helper() }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn read(source: Capture) -> i32 { val get = source.capture(Item { value: 1 }); get() }
        fn make() -> fn() -> i32 { val source: Capture = 7; source.capture(Item { value: 2 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
        fn main() -> i32 { read(7) }
    "#;
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("shared-reload", compile_test_bytecode(source))
        .unwrap();
    let interface = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let interface_root = runtime.root_value(interface).unwrap();
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
    let closure_root = runtime.root_value(closure).unwrap();
    let candidate = runtime
        .stage_reload_program(
            &old,
            "shared-reload",
            compile_test_bytecode(&source.replace("{ 42 }", "{ 43 }")),
        )
        .unwrap();
    let new = runtime.publish_staged_reload(candidate).unwrap();
    runtime.collect_garbage().unwrap();
    for (name, args, expected) in [
        ("read", vec![interface], 42),
        ("use_closure", vec![closure], 42),
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
    let retained = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(retained.contains(&old.key()));
}

#[test]
fn trapped_interface_frame_releases_its_roots_and_call_depth() {
    let (runtime, loaded) = load_test_module(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self / 0 } } fn main() -> i32 { 42 }",
    );
    let method = loaded.bytecode.interface_tables[0].methods[0].method;
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let vm = Vm::new(runtime);
    assert!(vm.invoke_interface_method(&boxed, &method, &[]).is_err());
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn interface_method_rejects_wrong_nominal_argument_before_execution() {
    let (runtime, loaded) = load_test_module(
        "struct A { val n: i32 } struct B { val n: i32 } trait Tag { fn read(self, x: A) -> i32; } impl Tag for i32 { fn read(self, x: A) -> i32 { x.n } } fn run<T: Tag>(x: T, a: A) -> i32 { x.read(a) } fn main() -> i32 { run(7, A { n: 1 }) } fn make_a() -> A { A { n: 5 } } fn make_b() -> B { B { n: 9 } }",
    );
    let method = loaded.bytecode.interface_tables[0].methods[0].method;
    let boxed = runtime.make_interface(&loaded, 0, Value::I32(7)).unwrap();
    let vm = Vm::new(runtime);
    let right = vm
        .execute(&loaded, "make_a")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    assert_eq!(
        vm.invoke_interface_method(&boxed, &method, &[right])
            .unwrap(),
        Value::I32(5)
    );
    let wrong = vm
        .execute(&loaded, "make_b")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
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
    use kagari_bytecode::{
        artifact::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact},
        instruction::InterfaceTableRef,
        program::{BytecodeProgram, ModuleRef},
        verifier::verify_module,
    };
    let module = interface_instruction_module();
    verify_module(&module).unwrap();

    let mut invalid = module.clone();
    invalid.functions[0].instructions[1] = BytecodeInstruction::MakeInterface {
        dst: Register::new(1),
        value: Register::new(0),
        module: ModuleRef::new(0),
        implementation: InterfaceTableRef::new(1),
        arguments: vec![],
    };
    assert!(verify_module(&invalid).is_err());
    invalid.functions[0].instructions[1] = BytecodeInstruction::MakeInterface {
        dst: Register::new(1),
        value: Register::new(1),
        module: ModuleRef::new(0),
        implementation: InterfaceTableRef::new(0),
        arguments: vec![],
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
    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime
        .load_program("interface-instruction", decoded.program)
        .unwrap();
    let vm = Vm::new(runtime);
    let value = vm
        .execute(&loaded, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    assert!(matches!(value, Value::Interface(_)));
    assert!(vm.runtime().gc().validate_value(&value));
}

#[test]
fn interface_instruction_uses_a_reachable_dependency_table() {
    use kagari_bytecode::{
        artifact::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact},
        instruction::InterfaceTableRef,
        program::{BytecodeProgram, ModuleRef, verify_program},
    };
    use kagari_common::identity::ModuleIdentity;

    let dependency = interface_instruction_module();
    let mut consumer = verified_module(
        vec![test_function(
            0,
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::MakeInterface {
                    dst: Register::new(1),
                    value: Register::new(0),
                    module: ModuleRef::new(0),
                    implementation: InterfaceTableRef::new(0),
                    arguments: vec![],
                },
                BytecodeInstruction::Return(Some(Register::new(1))),
            ],
            ValueType::HeapObject,
            vec![ValueType::I32, ValueType::HeapObject],
        )],
        vec![ConstantOperand::I32(11)],
    );
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

    let mut runtime = standard_runtime(Default::default());
    let loaded = runtime
        .load_program("interface-consumer", decoded.program)
        .unwrap();
    let dependency_key = loaded.member(ModuleRef::new(0)).unwrap().key();
    let vm = Vm::new(runtime);
    let value = vm
        .execute(&loaded, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    assert!(matches!(value, Value::Interface(_)));
    let _root = vm.runtime().root_value(value).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(dependency_key).is_some());
}

#[test]
fn a_retained_generic_closure_pins_the_callers_constraint_generation() {
    let source = r#"
        trait Capture {
            fn capture<F: Fn() -> i32>(self, callback: F) -> fn() -> i32 { || callback() }
        }
        impl Capture for i32 {}
        struct Callback {}
        impl Fn<()> for Callback {
            type Output = i32;
            fn call(self, args: ()) -> i32 { helper() }
        }
        fn helper() -> i32 { 42 }
        fn source() -> Capture { 7 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Callback {}) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
        fn main() -> i32 { helper() }
    "#;
    fn execute(runtime: &Runtime, module: &LoadedModule, name: &str, args: &[Value]) -> Value {
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
    let mut runtime = standard_runtime(Default::default());
    let first = runtime
        .load_program("constraint-reload", compile_test_bytecode(source))
        .unwrap();
    let receiver = execute(&runtime, &first, "source", &[]);
    let receiver_root = runtime.root_value(receiver).unwrap();
    let candidate = runtime
        .stage_reload_program(
            &first,
            "constraint-reload",
            compile_test_bytecode(&source.replace("{ 42 }", "{ 43 }")),
        )
        .unwrap();
    let second = runtime.publish_staged_reload(candidate).unwrap();
    let closure = execute(&runtime, &second, "make", &[receiver]);
    let closure_root = runtime.root_value(closure).unwrap();
    drop(receiver_root);
    let candidate = runtime
        .stage_reload_program(
            &second,
            "constraint-reload",
            compile_test_bytecode(&source.replace("{ 42 }", "{ 44 }")),
        )
        .unwrap();
    let third = runtime.publish_staged_reload(candidate).unwrap();
    let reclaimed = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(!reclaimed.contains(&first.key()));
    assert!(!reclaimed.contains(&second.key()));
    assert_eq!(
        execute(&runtime, &third, "use_closure", &[closure]),
        Value::I32(43)
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
    check_type_provenance_reload(source);
}

#[test]
fn shared_composite_arguments_keep_distinct_nominal_generations() {
    check_type_provenance_reload(
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
    );
}

#[test]
fn shared_closures_keep_type_metadata_without_retaining_caller_state() {
    check_type_provenance_reload(
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
    );
}

fn check_type_provenance_reload(source: &str) {
    let mut runtime = standard_runtime(Default::default());
    let old = runtime
        .load_program("type-provenance", compile_test_bytecode(source))
        .unwrap();
    let interface = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let interface_root = runtime.root_value(interface).unwrap();
    let second_source = source
        .replace("val value: i32", "val value: i32, val extra: bool")
        .replace("value: 1 }", "value: 1, extra: true }")
        .replace("{ 42 }", "{ 43 }");
    let candidate = runtime
        .stage_reload_program(
            &old,
            "type-provenance",
            compile_test_bytecode(&second_source),
        )
        .unwrap();
    let second = runtime.publish_staged_reload(candidate).unwrap();
    {
        let item_layout = |loaded: &LoadedModule| {
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
            runtime
                .alloc_struct(item_layout(&old), vec![Value::I32(1)])
                .unwrap(),
        );
        let old_root = runtime.root_value(old_item).unwrap();
        let current_layout = item_layout(&second);
        let item_type = Ty::Struct(NominalTy {
            declaration: current_layout.layout().declaration,
            arguments: vec![],
            associated_types: Default::default(),
        });
        let current_item = Value::Struct(
            runtime
                .alloc_struct(current_layout, vec![Value::I32(1), Value::Bool(true)])
                .unwrap(),
        );
        let current_root = runtime.root_value(current_item).unwrap();
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
            .validate_interface_method_arguments(&method, &[Value::I32(7), current_item])
            .unwrap();
        assert!(
            runtime
                .validate_interface_method_arguments(&method, &[Value::I32(7), old_item])
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
    let root = runtime.root_value(closure).unwrap();
    let third_source = second_source
        .replace("val extra: bool", "val extra: bool, val later: i32")
        .replace("extra: true }", "extra: true, later: 3 }")
        .replace("{ 43 }", "{ 44 }");
    let candidate = runtime
        .stage_reload_program(
            &second,
            "type-provenance",
            compile_test_bytecode(&third_source),
        )
        .unwrap();
    let third = runtime.publish_staged_reload(candidate).unwrap();
    let reclaimed = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(!reclaimed.contains(&old.key()));
    assert!(reclaimed.contains(&second.key()));
    assert!(runtime.validate_loaded_module(&second).is_err());
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
        Value::I32(42)
    );
    drop(root);
    drop(interface_root);
    let reclaimed = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(reclaimed.contains(&old.key()));
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn shared_aggregate_fields_retain_the_callers_nominal_generation() {
    check_type_provenance_reload(
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
    );
}

#[test]
fn shared_native_lists_retain_the_callers_nominal_generation() {
    check_type_provenance_reload(
        r#"use std::collections::{List};

        struct Item { val value: i32 }
        fn keep<T>(value: T) -> T { value }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                val list: List<T> = [value];
                || { val held: T = keep(list[0]); helper() }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Item { value: 1 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
    "#,
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
    let receiver = runtime.make_interface(&old, 0, Value::I32(7)).unwrap();
    let receiver_root = runtime.root_value(receiver).unwrap();
    let next_source = source
        .replace("val value: i32", "val value: i32, val extra: bool")
        .replace("value: 1 }", "value: 1, extra: true }");
    let candidate = runtime
        .stage_reload_program(&old, "closure-scope", compile_test_bytecode(&next_source))
        .unwrap();
    let current = runtime.publish_staged_reload(candidate).unwrap();
    let make = current
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "make")
        .unwrap()
        .id;
    let closure = Executor::new(&runtime, &current, make, slice::from_ref(&receiver))
        .unwrap()
        .run()
        .unwrap();
    let closure_root = runtime.root_value(closure).unwrap();
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
    for (owner, valid) in [(&old, false), (&current, true)] {
        let arguments = runtime
            .resolve_type_arguments(owner, slice::from_ref(&ty))
            .unwrap();
        let method = runtime
            .resolve_interface_method_slot(&receiver, interface, 0, &arguments)
            .unwrap();
        assert_eq!(
            runtime
                .validate_interface_method_result(&method, &closure)
                .is_ok(),
            valid
        );
    }
    drop((receiver_root, closure_root));
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
}

#[test]
fn shared_mutable_lists_preserve_mixed_nominal_scopes_and_parent_views() {
    check_type_provenance_reload(
        r#"use std::collections::{List, MutableList};

        struct Item { val value: i32 }
        struct Box<T> { val item: T }
        enum Wrapped<T> { Some(T), None }
        fn concrete() -> Box<Wrapped<Item>> { Box { item: Wrapped::Some(Item { value: 1 }) } }
        fn keep<T>(value: T) -> T { value }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                val list: MutableList<(Item, Box<Wrapped<T>>)> = [(Item { value: 1 }, Box { item: Wrapped::Some(value) })];
                list.push((Item { value: 1 }, Box { item: Wrapped::Some(value) }));
                val view: List<(Item, Box<Wrapped<T>>)> = list;
                var seen = 0;
                for pair in view {
                    val inner: T = match pair[1].item { Wrapped::Some(inner) => inner, Wrapped::None => value };
                    seen = seen + 1;
                }
                || {
                    val held: (Item, Box<Wrapped<T>>) = keep(view[1]);
                    match held[1].item { Wrapped::Some(inner) => { val again: T = keep(inner); helper() + seen - 2 }, Wrapped::None => 0 }
                }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Item { value: 1 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
    "#,
    );
}

#[test]
fn shared_repeat_arrays_keep_scalar_contracts_in_generic_frames() {
    check_type_provenance_reload(
        r#"use std::collections::{List};

        struct Item { val value: i32 }
        fn keep<T>(value: T) -> T { value }
        trait Capture {
            fn capture<T>(self, value: T) -> fn() -> i32 {
                val list: List<i32> = [42; 2];
                || { val held: i32 = keep(list[1]); helper() + held - 42 }
            }
        }
        impl Capture for i32 {}
        fn helper() -> i32 { 42 }
        fn make(source: Capture) -> fn() -> i32 { source.capture(Item { value: 1 }) }
        fn use_closure(call: fn() -> i32) -> i32 { call() }
    "#,
    );
}
