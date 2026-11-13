use super::fixture;
use super::{compile_without_foundation, fixture_without_foundation};
use crate::tests::{
    execution::native_boundary::{compile_program, publish_unused_version},
    support::bytecode::reload_bytecode,
};
use kagari_runtime::native::{conversion::context::ConversionContext, objects::Object};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;

use std::slice;

const SOURCE: &str =
    include_str!("../../../../../tests/fixtures/native/function-holder-applications.kgr");

#[test]
fn inherent_applications_infer_impl_arguments_and_check_method_evidence() {
    let (mut vm, owner) = fixture(SOURCE, None);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let runtime = &mut vm;
    let conversion = ConversionContext::new(runtime, &owner).unwrap();
    let i32_type = conversion.type_for::<i32>().unwrap();
    let string_type = conversion.type_for::<String>().unwrap();
    drop(conversion);
    let holder = runtime
        .bind_type(&_owner_program.root(), "Holder", slice::from_ref(&i32_type))
        .unwrap();
    let get = runtime.bind_method::<(), i32>(&holder, "get").unwrap();
    let replace = runtime
        .bind_method::<(i32,), i32>(&holder, "replace")
        .unwrap();
    let choose = runtime
        .bind_method_application::<(i32,), i32>(&holder, "choose", slice::from_ref(&i32_type))
        .unwrap();
    let again = runtime
        .bind_method_application_declaration::<(i32,), i32>(
            &holder.method("choose").unwrap(),
            slice::from_ref(&i32_type),
        )
        .unwrap();
    let identity = runtime
        .bind_associated_function_application::<(i32,), i32>(
            &holder,
            "identity",
            slice::from_ref(&i32_type),
        )
        .unwrap();
    let create = runtime
        .bind_associated_function::<(i32,), Object>(&holder, "create")
        .unwrap();
    assert!(
        runtime
            .bind_method::<(i32,), i32>(&holder, "choose")
            .is_err()
    );
    assert!(
        runtime
            .bind_method_application::<(String,), String>(
                &holder,
                "choose",
                slice::from_ref(&string_type)
            )
            .is_err()
    );
    assert!(
        runtime
            .bind_method_application::<(i32,), bool>(&holder, "choose", slice::from_ref(&i32_type))
            .is_err()
    );
    assert!(
        runtime
            .bind_method_application::<(i32,), i32>(
                &holder,
                "choose",
                &[i32_type.clone(), i32_type.clone()]
            )
            .is_err()
    );
    // A static signature that does not mention T still requires the exact impl application.
    let other = runtime
        .bind_type(&_owner_program.root(), "Holder", &[string_type])
        .unwrap();
    assert!(
        runtime
            .bind_associated_function_application::<(i32,), i32>(
                &other,
                "identity",
                slice::from_ref(&i32_type)
            )
            .is_err()
    );
    let object = runtime.call(&create, (42,)).unwrap();
    let mut cx = runtime.context(&owner).unwrap();
    assert_eq!(object.call(&mut cx, &get, ()).unwrap(), 42);
    assert_eq!(object.call(&mut cx, &replace, (11,)).unwrap(), 42);
    cx.collect_garbage().unwrap();
    assert_eq!(object.call(&mut cx, &get, ()).unwrap(), 11);
    assert_eq!(object.call(&mut cx, &choose, (7,)).unwrap(), 7);
    assert_eq!(object.call(&mut cx, &again, (8,)).unwrap(), 8);
    drop(cx);
    assert_eq!(runtime.call(&identity, (9,)).unwrap(), 9);
    drop((
        object, holder, get, replace, choose, again, identity, create, other,
    ));
    publish_unused_version(runtime, &_owner_program);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn inherent_application_handles_select_current_code_after_reload() {
    let source = SOURCE
        .replace("String", "bool")
        .replace("\"other\"", "true");
    let (mut vm, old) = fixture_without_foundation(&source, None);
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let i32_type = ConversionContext::new(&mut vm, &old)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let old_type = vm
        .bind_type(&_old_program.root(), "Holder", slice::from_ref(&i32_type))
        .unwrap();
    let old_get = vm.bind_method::<(), i32>(&old_type, "get").unwrap();
    let old_create = vm
        .bind_associated_function::<(i32,), Object>(&old_type, "create")
        .unwrap();
    let object = vm.call(&old_create, (42,)).unwrap();
    let source = source
        .replace("self.calls + 1", "self.calls + 10")
        .replace("Holder::create(42)", "Holder::create(43)");
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(&source, None),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    let new_type = vm
        .bind_type(&_new_program.root(), "Holder", &[i32_type])
        .unwrap();
    let new_get = vm.bind_method::<(), i32>(&new_type, "get").unwrap();
    let replacement: Object = vm
        .execute_typed(&_new_program, "make", (), &Default::default())
        .unwrap();
    let mut cx = vm.context(&new).unwrap();
    let other: Object = cx
        .runtime_mut()
        .execute_typed(&_new_program, "other", (), &Default::default())
        .unwrap();
    assert!(other.call(&mut cx, &old_get, ()).is_err());
    assert!(other.call(&mut cx, &new_get, ()).is_err());
    assert_eq!(object.call(&mut cx, &new_get, ()).unwrap(), 42);
    assert_eq!(replacement.call(&mut cx, &old_get, ()).unwrap(), 43);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(object.call(&mut cx, &old_get, ()).unwrap(), 42);
    assert_eq!(replacement.call(&mut cx, &new_get, ()).unwrap(), 43);
    let old_calls = cx
        .runtime_mut()
        .bind_field::<i32>(&old_type, "calls")
        .unwrap();
    let new_calls = cx
        .runtime_mut()
        .bind_field::<i32>(&new_type, "calls")
        .unwrap();
    assert_eq!(object.get(&mut cx, &old_calls).unwrap(), 20);
    assert_eq!(replacement.get(&mut cx, &new_calls).unwrap(), 20);
}

#[test]
fn nested_inherent_arguments_retain_nominal_results_and_selected_bounds() {
    let source = include_str!("../../../../../tests/fixtures/native/function-nested-holder.kgr");
    let (mut vm, owner) = fixture(source, None);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let runtime = &mut vm;
    let object: Object = runtime
        .execute_typed(&_owner_program, "make", (), &Default::default())
        .unwrap();
    let item = runtime
        .bind_type(&_owner_program.root(), "Item", &[])
        .unwrap();
    let first = runtime
        .bind_method::<(), Object>(object.object_type(), "first")
        .unwrap();
    let read = runtime
        .bind_method_application::<(Object,), i32>(
            object.object_type(),
            "read",
            slice::from_ref(item.type_argument()),
        )
        .unwrap();
    let wrong = ConversionContext::new(runtime, &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    assert!(
        runtime
            .bind_method_application::<(i32,), i32>(object.object_type(), "read", &[wrong])
            .is_err()
    );
    let mut cx = runtime.context(&owner).unwrap();
    let result = object.call(&mut cx, &first, ()).unwrap();
    cx.runtime_mut().collect_garbage().unwrap();
    let value = cx.runtime_mut().bind_field::<i32>(&item, "value").unwrap();
    result.set(&mut cx, &value, 43).unwrap();
    assert_eq!(object.call(&mut cx, &read, (result.clone(),)).unwrap(), 43);
    drop(object);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(result.get(&mut cx, &value).unwrap(), 43);
    drop((result, item, first, read, value));
    publish_unused_version(cx.runtime_mut(), &_owner_program);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn repeated_impl_parameters_reject_mixed_nominal_layout_scopes() {
    let source =
        include_str!("../../../../../tests/fixtures/native/function-pair-nominal-layout.kgr");
    let shared =
        include_str!("../../../../../tests/fixtures/native/function-pair-shared-layout.kgr");
    for (source, shared_layout) in [(source, false), (shared, true)] {
        let (mut vm, old) = fixture(source, None);
        let _old_program = old;
        let old = vm.select_program(&_old_program).unwrap();
        assert_eq!(
            old.bytecode
                .structures
                .iter()
                .any(|layout| layout.arguments.iter().any(|ty| !ty.is_concrete())),
            shared_layout
        );
        let old_item = vm.bind_type(&_old_program.root(), "Item", &[]).unwrap();
        let new = load_bytecode(
            &mut vm,
            "independent",
            compile_program(&source.replace("value: i32", "value: i64"), None),
        )
        .unwrap();
        let _new_program = new;
        let runtime = &mut vm;
        let new_item = runtime
            .bind_type(&_new_program.root(), "Item", &[])
            .unwrap();
        assert_eq!(old_item.type_argument().ty(), new_item.type_argument().ty());
        let pair = runtime
            .bind_type(
                &_old_program.root(),
                "Pair",
                &[
                    old_item.type_argument().clone(),
                    old_item.type_argument().clone(),
                ],
            )
            .unwrap();
        runtime.bind_method::<(), i32>(&pair, "same").unwrap();
        let mixed = runtime
            .bind_type(
                &_old_program.root(),
                "Pair",
                &[
                    old_item.type_argument().clone(),
                    new_item.type_argument().clone(),
                ],
            )
            .and_then(|mixed| runtime.bind_method::<(), i32>(&mixed, "same"));
        assert!(mixed.is_err());
    }
}
