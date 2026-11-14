use crate::tests::{execution::native_boundary::compile, support::invoke};
use kagari_runtime::{
    error::RuntimeErrorKind, module::entry::ProgramEntry,
    native::conversion::context::ConversionContext, value::kind::ObjectKind,
    value_semantics::identity_equal,
};

use std::ops::Bound;

#[test]
fn index_bounds_validate_the_applied_type_even_without_a_payload() {
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/index_bounds.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let valid = vm
        .execute_module(&loaded, "valid", vm.execution_options())
        .unwrap();
    {
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        let value = cx.import(&valid.return_value).unwrap();
        assert_eq!(cx.index_bound(&value).unwrap(), Bound::Unbounded);
    }
    for (name, expected) in [
        ("included", Bound::Included(u64::MAX)),
        ("excluded", Bound::Excluded(1_u64 << 32)),
    ] {
        let report = vm
            .execute_module(&loaded, name, vm.execution_options())
            .unwrap();
        vm.collect_garbage().unwrap();
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        let value = cx.import(&report.return_value).unwrap();
        assert_eq!(cx.index_bound(&value).unwrap(), expected);
    }
    for name in ["wrong_argument", "wrong_owner"] {
        let invalid = vm
            .execute_module(&loaded, name, vm.execution_options())
            .unwrap();
        vm.collect_garbage().unwrap();
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        let value = cx.import(&invalid.return_value).unwrap();
        assert_eq!(
            cx.index_bound(&value).unwrap_err().kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
    drop(valid);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(!vm.is_quarantined());
}

#[test]
fn assigns_stable_object_identity_and_kind() {
    let (mut vm, program) = compile(
        include_str!("../../../../tests/fixtures/native/gc_identity.kgr"),
        None,
    );
    let loaded = vm.select_program(&program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    let (first, second) = {
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        let tuple = cx.import(&report.return_value).unwrap();
        assert_eq!(cx.tuple_len(&tuple).unwrap(), 2);
        let first = cx.tuple_element(&tuple, 0).unwrap();
        let second = cx.tuple_element(&tuple, 1).unwrap();
        assert_eq!(first.kind(), Some(ObjectKind::Array));
        assert_eq!(second.kind(), Some(ObjectKind::Struct));
        (cx.retain(&first).unwrap(), cx.retain(&second).unwrap())
    };
    let alias = first.clone();
    assert!(!identity_equal(&mut vm, &first, &second).unwrap());
    drop(report);
    vm.collect_garbage().unwrap();
    assert!(identity_equal(&mut vm, &first, &alias).unwrap());
    assert!(!identity_equal(&mut vm, &first, &second).unwrap());
    let name = vm.reflect_type_of(&first).unwrap();
    assert_eq!(
        ConversionContext::new(&mut vm, &loaded)
            .unwrap()
            .decode::<String>(&name)
            .unwrap(),
        "array"
    );
    drop((first, second, alias, name));
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn root_scanning_traces_only_gc_managed_boundaries() {
    let (mut vm, program) = compile(
        include_str!("../../../../tests/fixtures/native/gc_root_boundaries.kgr"),
        None,
    );
    let loaded = vm.select_program(&program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let root = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value;
    vm.collect_garbage().unwrap();
    // The revision already owns its two String constants. The result adds one
    // tuple, record, leaf array and String array; the unit field adds no object.
    assert_eq!(vm.gc_stats().objects, baseline.objects + 4);
    let (record, strings) = {
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        let tuple = cx.import(&root).unwrap();
        assert_eq!(cx.tuple_len(&tuple).unwrap(), 3);
        assert!(cx.tuple_element(&tuple, 2).unwrap().is_unit());
        let record = cx.tuple_element(&tuple, 0).unwrap();
        let strings = cx.tuple_element(&tuple, 1).unwrap();
        (cx.retain(&record).unwrap(), cx.retain(&strings).unwrap())
    };
    let leaf = invoke(&mut vm, &loaded, "leaf", std::slice::from_ref(&record));
    let name = invoke(&mut vm, &loaded, "name", std::slice::from_ref(&record));
    {
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        assert_eq!(cx.decode::<String>(&name).unwrap(), "leaf");
        let values = cx.import(&strings).unwrap();
        assert_eq!(cx.array_len(&values).unwrap(), 1);
        let seen = cx.array_element(&values, 0usize).unwrap().unwrap();
        let text = cx.type_for::<String>().unwrap();
        assert_eq!(cx.decode_value::<String>(&text, &seen).unwrap(), "seen");
    }
    drop((root, record, strings, name));
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects + 1);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations + 1
    );
    {
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        let value = cx.import(&leaf).unwrap();
        assert_eq!(cx.array_len(&value).unwrap(), 1);
        assert_eq!(
            cx.array_element(&value, 0usize).unwrap().unwrap().as_i32(),
            Some(1)
        );
    }
    drop(leaf);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn root_scanning_handles_cycles_without_duplicate_identity() {
    let (mut vm, program) = compile(
        include_str!("../../../../tests/fixtures/native/gc_cycle_roots.kgr"),
        None,
    );
    let loaded = vm.select_program(&program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let root = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value;
    vm.collect_garbage().unwrap();
    let live_objects = vm.gc_stats().objects;
    assert_eq!(live_objects, baseline.objects + 2);
    let next = invoke(&mut vm, &loaded, "next", std::slice::from_ref(&root));
    assert!(identity_equal(&mut vm, &root, &next).unwrap());
    drop(root);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, live_objects);
    let another = invoke(&mut vm, &loaded, "next", std::slice::from_ref(&next));
    assert!(identity_equal(&mut vm, &next, &another).unwrap());
    drop((next, another));
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn custom_set_absent_removal_does_not_invalidate_iteration() {
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/set-absent-removal.kgr"),
        None,
    );
    let loaded = vm.select_program(&loaded).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(1)
    );
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(vm.gc_stats().objects, baseline.objects);
}
