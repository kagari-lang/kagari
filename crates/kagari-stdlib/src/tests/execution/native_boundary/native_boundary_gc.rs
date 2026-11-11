#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::{declarations::StandardDeclarations, tests::support::bytecode::reload_bytecode};
mod payload_failures;
mod program_cycles;
use super::{compile, compile_program};
use kagari_runtime::{
    error::RuntimeError,
    gc::roots::RootedValue,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        types::Type,
    },
    value::Value,
    value_semantics::format_value,
};

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

#[test]
fn mark_sweep_traces_tuples_enum_payloads_and_cycles_without_retaining_unreachable_graphs() {
    let (vm, loaded) = compile(
        r#"use std::collections::{HashMap, HashSet};

        struct Node { val edges: HashMap<i32, Node> }
        fn main() -> Option<(Vec<Node>,)> {
            val edges: HashMap<i32, Node> = HashMap::new();
            val node = Node { edges: edges };
            edges.insert(1, node);
            Some(([node],))
        }
        pub fn garbage() -> HashSet<i32> { val set: HashSet<i32> = HashSet::new(); set.insert(1); set }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let value = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let root = vm
        .root_value(vm.gc().alloc_tuple(vec![value]).unwrap())
        .unwrap();
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 6);
    let live_units = vm.gc().stats().current_heap_units;
    assert_eq!(live_units, 12);
    vm.execute_module(&loaded, "garbage", vm.execution_options())
        .unwrap();
    let collection = vm.collect_garbage().unwrap();
    assert_eq!(
        (collection.reclaimed_objects, collection.live_objects),
        (1, 6)
    );
    assert_eq!(vm.gc().stats().current_heap_units, live_units);
    root.set(vm.gc(), Value::Unit).unwrap();
    let collection = vm.collect_garbage().unwrap();
    assert_eq!(
        (collection.reclaimed_objects, collection.live_objects),
        (6, 0)
    );
    assert_eq!(vm.gc().stats().current_heap_units, 0);
}

#[test]
fn tracing_a_deep_heap_chain_uses_an_explicit_work_stack() {
    let (vm, loaded) = compile(
        r#"
        struct Node { val next: Option<Node> }
        fn main() -> Option<Node> {
            var head: Option<Node> = None;
            for index in 0..10000 { head = Some(Node { next: head }); }
            head
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let value = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let root = vm.root_value(value).unwrap();
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 20_001);
    drop(root);
    assert_eq!(vm.collect_garbage().unwrap().reclaimed_objects, 20_001);
}

#[test]
fn intrinsic_formatting_is_bounded_and_does_not_read_mutable_graphs() {
    let (vm, loaded) = compile(
        r#"
        struct Node { var next: Option<Node> }
        fn main() -> Node {
            val node = Node { next: None };
            node.next = Some(node);
            node
        }
    "#,
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let Value::Struct(object) = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result")
    else {
        panic!("array")
    };
    let runtime = &vm;
    let preview = format_value(runtime.gc(), &Value::Struct(object), true).unwrap();
    assert!(preview.starts_with("Struct@"));
    assert_eq!(
        format_value(
            runtime.gc(),
            &vm.gc().alloc_string("a\nb".into()).unwrap(),
            true
        )
        .unwrap(),
        "\"a\\nb\""
    );
    assert!(
        format_value(
            runtime.gc(),
            &vm.gc().alloc_string("x".repeat(1_048_577)).unwrap(),
            false
        )
        .is_err()
    );
    let mut value = Value::I32(42);
    for _ in 0..66 {
        value = vm.gc().alloc_tuple(vec![value]).unwrap();
    }
    assert!(format_value(runtime.gc(), &value, true).is_err());
    assert!(runtime.gc().struct_layout(object).is_some());
}

#[test]
fn rooted_data_keeps_type_metadata_without_retaining_obsolete_module_state() {
    let captured = Arc::new(Mutex::new(None::<RootedValue>));
    let mut builder = ModuleBuilder::new(
        "fixture::capture",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let capture = builder
        .define_function(FunctionDecl::new("capture"))
        .unwrap();
    builder
        .function(&capture, |function| {
            let value = function.type_parameter("T")?.ty();
            function.parameter("value", value)
        })
        .unwrap();
    let capture_slot = captured.clone();
    builder
        .bind(
            capture,
            NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::unit()), move |cx| {
                let root = cx
                    .heap()
                    .root_value(cx.argument(0)?)
                    .ok_or_else(|| RuntimeError::module_validation("captured data root"))?;
                let mut slot = capture_slot.lock().unwrap();
                assert!(slot.is_none());
                *slot = Some(root);
                Ok(Value::Unit)
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, old) = compile(
        r#"use fixture::capture::capture;
        struct Item { val value: i32 }
        fn main() -> i32 {
            capture([Item { value: 42 }]);
            42
        }"#,
        Some(&module),
    );
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    assert_eq!(
        vm.execute_module(&old, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .value(vm.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    // Transfer the sole data lease out of the native capture; retain no callable metadata.
    let root = captured.lock().unwrap().take().unwrap();
    assert!(captured.lock().unwrap().is_none());
    assert_eq!(vm.gc().active_roots(), 1);
    let Value::Array(array) = root.value(vm.gc()).unwrap() else {
        panic!("typed array")
    };
    reload_bytecode(&mut vm,
        &_old_program,
        Some("boundary".into()),
        compile_program(
            "use fixture::capture::capture; struct Item { val value: i32 } pub fn retained_recipe() { capture([Item { value: 42 }]); } fn main() -> i32 { 99 }",
            Some(&module),
        ),
    )
    .unwrap();
    let _replacement_program = _old_program.clone();
    let replacement = vm.select_program(&_replacement_program).unwrap();
    let expected: HashSet<_> = old.members().map(|member| member.key()).collect();
    let reclaimed: HashSet<_> = vm
        .collect_garbage()
        .unwrap()
        .reclaimed_modules
        .into_iter()
        .collect();
    assert_eq!(reclaimed, expected);
    assert!(vm.validate_executable_module(&old).is_err());
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 2);
    let item = vm.gc().array_get(array, 0).unwrap();
    vm.gc().array_set(array, 0, item).unwrap();
    assert!(vm.gc().array_set(array, 0, Value::I32(7)).is_err());
    assert_eq!(vm.gc().array_len(array), Some(1));
    assert_eq!(
        vm.execute_module(&replacement, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .value(vm.gc())
            .expect("retained execution result"),
        Value::I32(99)
    );
    drop(root);
    assert_eq!(vm.gc().active_roots(), 0);
    assert_eq!(vm.collect_garbage().unwrap().reclaimed_objects, 2);
}

#[test]
fn recursive_element_contracts_reject_foreign_nested_layouts_after_reload() {
    let source = r#"
        struct Inner { val value: i32 }
        struct Node { var next: Option<Node>, val value: Inner }
        fn main() -> [Node] {
            val node = Node { next: None, value: Inner { value: 42 } };
            node.next = Some(node);
            [node]
        }
    "#;
    let (mut vm, old) = compile(source, None);
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let old_value = vm
        .execute_module(&old, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let old_root = vm.root_value(old_value).unwrap();
    let replacement = source
        .replace("val value: i32", "val value: i32, val extra: bool")
        .replace("value: 42 }", "value: 42, extra: true }");
    assert!(
        reload_bytecode(
            &mut vm,
            &_old_program,
            None,
            compile_program(&replacement, None)
        )
        .is_err()
    );
    reload_bytecode(
        &mut vm,
        &_old_program,
        None,
        compile_program(&source.replace("42", "43"), None),
    )
    .unwrap();
    let independent = load_bytecode(
        &mut vm,
        "foreign-nested-layout",
        compile_program(&replacement, None),
    )
    .unwrap();
    let current = vm.select_program(&independent).unwrap();
    let new_value = vm
        .execute_module(&current, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let new_root = vm.root_value(new_value).unwrap();
    let (Value::Array(old_array), Value::Array(new_array)) = (old_value, new_value) else {
        panic!("typed arrays");
    };
    let reclaimed = vm.collect_garbage().unwrap().reclaimed_modules;
    assert!(
        old.members()
            .all(|member| reclaimed.contains(&member.key()))
    );
    let old_node = vm.gc().array_get(old_array, 0).unwrap();
    let new_node = vm.gc().array_get(new_array, 0).unwrap();
    assert!(vm.gc().array_set(old_array, 0, new_node).is_err());
    assert!(vm.gc().array_set(new_array, 0, old_node).is_err());
    assert_eq!(vm.gc().array_len(old_array), Some(1));
    assert_eq!(vm.gc().array_len(new_array), Some(1));
    vm.gc().array_set(old_array, 0, old_node).unwrap();
    vm.gc().array_set(new_array, 0, new_node).unwrap();
    drop((old_root, new_root));
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
}
