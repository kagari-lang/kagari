use super::fixture;
use super::{compile_without_foundation, fixture_without_foundation};
use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::publish_unused_version, support::bytecode::reload_bytecode,
    },
};

use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, conversion::context::ConversionContext,
    declarations::FunctionDecl, function_handle::Function, interfaces::Interface, objects::Object,
    typed::NativeContext,
};

use std::{
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const IDENTITY: &str =
    include_str!("../../../../../tests/fixtures/native/interface-inherited-applications.kgr");

#[test]
fn inherited_generic_interface_bindings_require_checked_applications_and_keep_returns_alive() {
    let (mut vm, owner) = fixture(IDENTITY, None);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let target: Interface = vm
        .execute_typed(&_owner_program, "make", (), &Default::default())
        .unwrap();
    let item: Object = vm
        .execute_typed(&_owner_program, "item", (), &Default::default())
        .unwrap();
    let mut conversion = ConversionContext::new(&mut vm, &owner).unwrap();
    let integer = conversion.type_for::<i32>().unwrap();
    let boolean = conversion.type_for::<bool>().unwrap();
    let identity_member = target.member(conversion.runtime_mut(), "identity").unwrap();
    assert!(
        conversion
            .runtime_mut()
            .bind_interface_method::<(i32,), i32>(&identity_member)
            .is_err()
    );
    assert!(
        conversion
            .runtime_mut()
            .bind_interface_method_application::<(bool,), bool>(&identity_member, &[boolean])
            .is_err()
    );
    assert!(
        conversion
            .runtime_mut()
            .bind_interface_method_application::<(i32,), bool>(
                &identity_member,
                slice::from_ref(&integer)
            )
            .is_err()
    );
    let scalar = conversion
        .runtime_mut()
        .bind_interface_method_application::<(i32,), i32>(&identity_member, &[integer])
        .unwrap();
    let object = conversion
        .runtime_mut()
        .bind_interface_method_application::<(Object,), Object>(
            &identity_member,
            slice::from_ref(item.object_type().type_argument()),
        )
        .unwrap();
    let delayed_member = target.member(conversion.runtime_mut(), "delayed").unwrap();
    let delayed = conversion
        .runtime_mut()
        .bind_interface_method_application::<(Object,), Function<(), Object>>(
            &delayed_member,
            slice::from_ref(item.object_type().type_argument()),
        )
        .unwrap();
    let mut cx = conversion.runtime_mut().context(&owner).unwrap();
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(target.call(&mut cx, &scalar, (42,)).unwrap(), 42);
    let returned = target.call(&mut cx, &object, (item.clone(),)).unwrap();
    let field = cx
        .runtime_mut()
        .bind_field::<i32>(item.object_type(), "value")
        .unwrap();
    returned.set(&mut cx, &field, 43).unwrap();
    assert_eq!(item.get(&mut cx, &field).unwrap(), 43);
    let closure = target.call(&mut cx, &delayed, (item,)).unwrap();
    drop(returned);
    drop(target);
    cx.runtime_mut().collect_garbage().unwrap();
    let returned = cx.runtime_mut().call(&closure, ()).unwrap();
    assert_eq!(returned.get(&mut cx, &field).unwrap(), 43);
    drop(returned);
    drop(closure);
    drop(scalar);
    drop(object);
    drop(delayed);
    drop(field);
    drop(identity_member);
    drop(delayed_member);
    drop(cx);
    drop(conversion);
    // Generic operation descriptions belong to the actual receiver revision.
    // Retire that warmed owner before comparing the cold publication baseline.
    publish_unused_version(&mut vm, &_owner_program);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn generic_interface_constraints_keep_selected_operations_alive_through_gc_and_reload() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut builder = ModuleBuilder::new(
        "example::interface_applications",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    {
        let registered_function = builder
            .define_function(FunctionDecl::new("collect"))
            .unwrap();
        builder
            .bind(
                registered_function.clone(),
                move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                    cx.collect_garbage()?;
                    observed.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let module = builder.finish().unwrap();
    let source =
        include_str!("../../../../../tests/fixtures/native/interface-application-reload.kgr");
    let (mut vm, old) = fixture_without_foundation(source, Some(&module));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let target: Interface = vm
        .execute_typed(&_old_program, "make", (), &Default::default())
        .unwrap();
    let key: Object = vm
        .execute_typed(&_old_program, "key", (), &Default::default())
        .unwrap();
    let member = target.member(&mut vm, "invoke").unwrap();
    let invoke = vm
        .bind_interface_method_application::<(Object,), i32>(
            &member,
            slice::from_ref(key.object_type().type_argument()),
        )
        .unwrap();
    let duplicate = vm
        .bind_interface_method_application::<(Object,), i32>(
            &member,
            slice::from_ref(key.object_type().type_argument()),
        )
        .unwrap();
    drop(member);
    vm.collect_garbage().unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        target
            .call(&mut vm.context(&old).unwrap(), &invoke, (key.clone(),))
            .unwrap(),
        42
    );
    assert_eq!(source.matches("self.value").count(), 1);
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(
            &source.replace("self.value", "self.value + 1"),
            Some(&module),
        ),
    )
    .unwrap();
    let new = vm.select_program(&_old_program).unwrap();
    let new_target: Interface = vm
        .execute_typed(&_old_program, "make", (), &Default::default())
        .unwrap();
    let new_key: Object = vm
        .execute_typed(&_old_program, "key", (), &Default::default())
        .unwrap();
    let new_member = new_target.member(&mut vm, "invoke").unwrap();
    let new_invoke = vm
        .bind_interface_method_application::<(Object,), i32>(
            &new_member,
            slice::from_ref(new_key.object_type().type_argument()),
        )
        .unwrap();
    let mut cx = vm.context(&new).unwrap();
    let wrong_key: Object = cx
        .runtime_mut()
        .execute_typed(&_old_program, "wrong_key", (), &Default::default())
        .unwrap();
    assert!(target.call(&mut cx, &invoke, (wrong_key,)).is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        new_target
            .call(&mut cx, &invoke, (new_key.clone(),))
            .unwrap(),
        43
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(target.call(&mut cx, &invoke, (key.clone(),)).unwrap(), 43);
    assert_eq!(
        new_target.call(&mut cx, &new_invoke, (new_key,)).unwrap(),
        43
    );
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    drop((invoke, target, key));
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        duplicate.member().owner().program_root().key(),
        old.program_root().key()
    );
    assert!(
        cx.runtime().validate_executable_module(&old).is_ok(),
        "the member owns its actual lexical revision"
    );
    drop(duplicate);
    // The scalar receiver's current dispatch needs no persistent old environment.
    // The last scoped member lease, rather than a duplicate cache, owns retirement.
    cx.runtime_mut().collect_garbage().unwrap();
    assert!(cx.runtime().validate_executable_module(&old).is_err());
    drop(cx);
    reload_bytecode(
        &mut vm,
        &_old_program,
        None,
        compile_without_foundation(
            &source.replace("self.value", "self.value + 1"),
            Some(&module),
        ),
    )
    .unwrap();
    drop((new_invoke, new_target, new_member));
    vm.collect_garbage().unwrap();
    assert!(vm.validate_executable_module(&old).is_err());
    assert!(vm.validate_executable_module(&new).is_err());
}
