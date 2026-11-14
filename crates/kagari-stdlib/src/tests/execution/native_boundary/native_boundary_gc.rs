#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::{declarations::StandardDeclarations, tests::support::bytecode::reload_bytecode};
mod payload_failures;
mod program_cycles;
use super::{compile, compile_program};
use kagari_runtime::{
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        conversion::context::ConversionContext,
        declarations::FunctionDecl,
        types::Type,
    },
    value::{carriers::RootedValue, view::ValueView},
    value_semantics::format_value,
};

use std::sync::{Arc, Mutex};

#[test]
fn mark_sweep_traces_tuples_enum_payloads_and_cycles_without_retaining_unreachable_graphs() {
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/tuple-enum-cycle.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let root = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value;
    vm.collect_garbage().unwrap();
    let live = vm.gc_stats();
    assert!(live.objects > baseline.objects);
    assert!(live.provider_units > baseline.provider_units);
    assert_eq!(
        vm.execute(
            &_loaded_program,
            "inspect",
            std::slice::from_ref(&root),
            &Default::default()
        )
        .unwrap()
        .return_value
        .as_i32(),
        Some(42)
    );
    drop(
        vm.execute_module(&loaded, "garbage", vm.execution_options())
            .unwrap(),
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, live.objects);
    assert_eq!(vm.gc_stats().provider_units, live.provider_units);
    drop(root);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(vm.gc_stats().provider_units, baseline.provider_units);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn tracing_a_deep_heap_chain_uses_an_explicit_work_stack() {
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/deep-chain.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let root = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value;
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects + 20_001);
    drop(root);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn intrinsic_formatting_is_bounded_and_does_not_read_mutable_graphs() {
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/mutable-format.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let object = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value;
    let preview = format_value(&mut vm, &object, true).unwrap();
    assert!(preview.starts_with("Struct@"));
    vm.collect_garbage().unwrap();
    let next = vm.reflect_get_field(&object, "next").unwrap();
    assert_eq!(format_value(&mut vm, &object, true).unwrap(), preview);
    assert!(
        format_value(&mut vm, &next, true)
            .unwrap()
            .starts_with("Option::Some(")
    );
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
                let root = cx.value_handle(0)?.root(cx)?;
                let mut slot = capture_slot.lock().unwrap();
                assert!(slot.is_none());
                *slot = Some(root);
                Ok(ValueView::unit())
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, old) = compile(
        include_str!("../../../../tests/fixtures/native/retained-data.kgr"),
        Some(&module),
    );
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    assert_eq!(
        vm.execute_module(&old, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    // Transfer the sole data lease out of the native capture; retain no callable metadata.
    let root = captured.lock().unwrap().take().unwrap();
    assert!(captured.lock().unwrap().is_none());
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("boundary".into()),
        compile_program(
            include_str!("../../../../tests/fixtures/native/retained-data-reloaded.kgr"),
            Some(&module),
        ),
    )
    .unwrap();
    let _replacement_program = _old_program.clone();
    let replacement = vm.select_program(&_replacement_program).unwrap();
    vm.collect_garbage().unwrap();
    for member in old.members() {
        assert!(vm.modules().loaded(member.key()).is_none());
    }
    assert!(vm.validate_executable_module(&old).is_err());
    let live = vm.gc_stats();
    {
        let mut conversion = ConversionContext::new(&mut vm, &replacement).unwrap();
        let array = conversion.import(&root).unwrap();
        let item = conversion.array_element(&array, 0).unwrap().unwrap();
        conversion.array_set(&array, 0, &item).unwrap();
        assert!(
            conversion
                .array_set(&array, 0, &ValueView::from(7i32))
                .is_err()
        );
        assert_eq!(conversion.array_len(&array).unwrap(), 1);
        let child = conversion.retain(&item).unwrap();
        assert_eq!(
            conversion
                .runtime_mut()
                .reflect_get_field(&child, "value")
                .unwrap()
                .as_i32(),
            Some(42)
        );
    }
    assert_eq!(
        vm.execute_module(&replacement, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(99)
    );
    drop(root);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, live.objects - 2);
    assert_eq!(
        vm.gc_stats().root_registrations,
        live.root_registrations - 1
    );
}

#[test]
fn recursive_element_contracts_reject_foreign_nested_layouts_after_reload() {
    let source = include_str!("../../../../tests/fixtures/native/nested-layout.kgr");
    let (mut vm, old) = compile(source, None);
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let old_value = vm
        .execute_module(&old, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value;
    let old_root = old_value;
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
        .return_value;
    let new_root = new_value;
    vm.collect_garbage().unwrap();
    assert!(
        old.members()
            .all(|member| vm.modules().loaded(member.key()).is_none())
    );
    let live = vm.gc_stats();
    {
        let mut conversion = ConversionContext::new(&mut vm, &current).unwrap();
        let old_array = conversion.import(&old_root).unwrap();
        let new_array = conversion.import(&new_root).unwrap();
        let old_node = conversion.array_element(&old_array, 0).unwrap().unwrap();
        let new_node = conversion.array_element(&new_array, 0).unwrap().unwrap();
        assert!(conversion.array_set(&old_array, 0, &new_node).is_err());
        assert!(conversion.array_set(&new_array, 0, &old_node).is_err());
        assert_eq!(conversion.array_len(&old_array).unwrap(), 1);
        assert_eq!(conversion.array_len(&new_array).unwrap(), 1);
        conversion.array_set(&old_array, 0, &old_node).unwrap();
        conversion.array_set(&new_array, 0, &new_node).unwrap();
    }
    drop((old_root, new_root));
    vm.collect_garbage().unwrap();
    // Each recursive graph has its array, Node, Some(Node), and scalar Inner.
    assert_eq!(vm.gc_stats().objects, live.objects - 8);
    assert_eq!(
        vm.gc_stats().root_registrations,
        live.root_registrations - 2
    );
}
