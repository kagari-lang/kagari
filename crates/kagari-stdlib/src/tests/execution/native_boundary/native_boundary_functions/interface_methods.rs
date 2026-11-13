use super::fixture;
use super::{compile_without_foundation, fixture_without_foundation};
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, declarations::FunctionDecl,
    interfaces::Interface, objects::Object, typed::NativeContext,
};

use crate::{declarations::StandardDeclarations, tests::support::bytecode::reload_bytecode};
use kagari_common::cancellation::CancellationToken;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const SOURCE: &str =
    include_str!("../../../../../../kagari-runtime/tests/fixtures/execution/interface-methods.kgr");

#[test]
fn prepared_interface_members_call_inherited_and_default_methods_on_multiple_receivers() {
    let (mut vm, owner) = fixture(SOURCE, None);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let player: Interface = vm
        .execute_typed(&_owner_program, "make", (100,), &Default::default())
        .unwrap();
    let second: Interface = vm
        .execute_typed(&_owner_program, "make", (40,), &Default::default())
        .unwrap();
    let member = player.member(&mut vm, "get").unwrap();
    let direct = player
        .member_declaration(
            &mut vm,
            &member
                .definitions()
                .resolve(member.declaration())
                .unwrap()
                .to_path(),
        )
        .unwrap();
    let get = vm.bind_interface_method::<(), i32>(&direct).unwrap();
    assert!(vm.bind_interface_method::<(i32,), i32>(&direct).is_err());
    assert!(vm.bind_interface_method::<(), bool>(&direct).is_err());
    let damage_member = player.member(&mut vm, "damage").unwrap();
    let damage = vm
        .bind_interface_method::<(i32,), i32>(&damage_member)
        .unwrap();
    assert!(player.member(&mut vm, "missing").is_err());
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(player.call(&mut cx, &get, ()).unwrap(), 100);
    assert_eq!(second.call(&mut cx, &get, ()).unwrap(), 40);
    assert_eq!(player.call(&mut cx, &damage, (10,)).unwrap(), 90);
    let member = player.member(cx.runtime_mut(), "fail").unwrap();
    let fail = cx
        .runtime_mut()
        .bind_interface_method::<(i32,), i32>(&member)
        .unwrap();
    assert!(player.call(&mut cx, &fail, (0,)).is_err());
    assert_eq!(cx.runtime().resources().counters().current_call_depth, 0);
    let token = CancellationToken::default();
    token.cancel();
    let mut options = cx.runtime().execution_options();
    options.cancellation = token;
    assert!(
        player
            .call_with_options(&mut cx, &get, (), options)
            .is_err()
    );
    assert_eq!(player.call(&mut cx, &get, ()).unwrap(), 100);
    let (mut foreign, foreign_owner) = fixture(SOURCE, None);
    let _foreign_owner_program = foreign_owner;
    let foreign_owner = foreign.select_program(&_foreign_owner_program).unwrap();
    assert!(
        player
            .call(&mut foreign.context(&foreign_owner).unwrap(), &get, ())
            .is_err()
    );
}

#[test]
fn interface_method_results_use_existing_native_and_inherited_adapters() {
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/interface-native-adapters.kgr"),
        None,
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let list: Interface = vm
        .execute_typed(&_owner_program, "make", (), &Default::default())
        .unwrap();
    let registered = StandardDeclarations::default()
        .list()
        .method("len")
        .unwrap();
    let direct = vm
        .interface_member_declaration(
            &owner,
            list.type_argument(),
            &registered
                .definitions()
                .resolve(registered.id())
                .unwrap()
                .to_path(),
        )
        .unwrap();
    let len = vm.bind_interface_method::<(), usize>(&direct).unwrap();
    let iter_member = list.member(&mut vm, "iter").unwrap();
    let iter = vm
        .bind_interface_method::<(), Interface>(&iter_member)
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(list.call(&mut cx, &len, ()).unwrap(), 2);
    let iterator = list.call(&mut cx, &iter, ()).unwrap();
    let member = iterator.member(cx.runtime_mut(), "next").unwrap();
    let next = cx
        .runtime_mut()
        .bind_interface_method::<(), Option<i32>>(&member)
        .unwrap();
    drop(list);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(iterator.call(&mut cx, &next, ()).unwrap(), Some(20));
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(iterator.call(&mut cx, &next, ()).unwrap(), Some(22));
    assert_eq!(iterator.call(&mut cx, &next, ()).unwrap(), None);
    drop(iterator);
    drop(member);
    drop(next);
    drop(iter_member);
    drop(iter);
    drop(direct);
    drop(len);
    drop(cx);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn interface_calls_preserve_associated_object_outputs_and_public_access() {
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/interface-associated-objects.kgr"),
        None,
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let value: Interface = vm
        .execute_typed(&_owner_program, "make", (), &Default::default())
        .unwrap();
    let hidden: Interface = vm
        .execute_typed(&_owner_program, "hidden", (), &Default::default())
        .unwrap();
    assert!(hidden.member(&mut vm, "read").is_err());
    let read_member = value.member(&mut vm, "read").unwrap();
    let read = vm
        .bind_interface_method::<(), Object>(&read_member)
        .unwrap();
    let result = value
        .call(&mut vm.context(&owner).unwrap(), &read, ())
        .unwrap();
    drop(value);
    drop(hidden);
    vm.collect_garbage().unwrap();
    let field = vm.bind_field::<i32>(result.object_type(), "value").unwrap();
    assert_eq!(
        result
            .get(&mut vm.context(&owner).unwrap(), &field)
            .unwrap(),
        42
    );
    drop(result);
    drop(field);
    drop(read);
    drop(read_member);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn interface_dispatch_collects_during_reentry_and_selects_current_default_code() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut builder = ModuleBuilder::new(
        "example::interface_methods",
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
                    observed.fetch_add(1, Ordering::Relaxed);
                    cx.collect_garbage()?;
                    Ok(())
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let module = builder.finish().unwrap();
    assert_eq!(SOURCE.matches("self.get() - amount").count(), 1);
    let source = format!(
        "use example::interface_methods::collect;\n{}",
        SOURCE.replace("self.get() - amount", "collect(); self.get() - amount")
    );
    let (mut vm, old) = fixture_without_foundation(&source, Some(&module));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let player: Interface = vm
        .execute_typed(&_old_program, "make", (100,), &Default::default())
        .unwrap();
    let member = player.member(&mut vm, "damage").unwrap();
    let damage = vm.bind_interface_method::<(i32,), i32>(&member).unwrap();
    let duplicate = vm.bind_interface_method::<(i32,), i32>(&member).unwrap();
    drop(member);
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(
            &source.replace("self.get() - amount", "self.get() - amount - 1"),
            Some(&module),
        ),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    let new_player: Interface = vm
        .execute_typed(&_new_program, "make", (100,), &Default::default())
        .unwrap();
    let new_member = new_player.member(&mut vm, "damage").unwrap();
    let new_damage = vm
        .bind_interface_method::<(i32,), i32>(&new_member)
        .unwrap();
    let mut cx = vm.context(&new).unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(new_player.call(&mut cx, &damage, (10,)).unwrap(), 89);
    assert_eq!(player.call(&mut cx, &new_damage, (10,)).unwrap(), 89);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(player.call(&mut cx, &damage, (10,)).unwrap(), 89);
    assert_eq!(new_player.call(&mut cx, &new_damage, (10,)).unwrap(), 89);
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    drop(player);
    drop(damage);
    cx.runtime_mut().collect_garbage().unwrap();
    assert!(cx.runtime().validate_executable_module(&old).is_ok());
    drop(duplicate);
    cx.runtime_mut().collect_garbage().unwrap();
    assert!(cx.runtime().validate_executable_module(&old).is_err());
}
