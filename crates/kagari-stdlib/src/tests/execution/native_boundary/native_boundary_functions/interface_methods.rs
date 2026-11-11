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
    let (vm, owner) = fixture(SOURCE, None);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let player: Interface = vm
        .execute_typed(&_owner_program, "make", (100,), &Default::default())
        .unwrap();
    let second: Interface = vm
        .execute_typed(&_owner_program, "make", (40,), &Default::default())
        .unwrap();
    let member = player.member(&vm, "get").unwrap();
    let direct = player
        .member_declaration(
            &vm,
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
    let damage = vm
        .bind_interface_method::<(i32,), i32>(&player.member(&vm, "damage").unwrap())
        .unwrap();
    assert!(player.member(&vm, "missing").is_err());
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(player.call(&mut cx, &get, ()).unwrap(), 100);
    assert_eq!(second.call(&mut cx, &get, ()).unwrap(), 40);
    assert_eq!(player.call(&mut cx, &damage, (10,)).unwrap(), 90);
    let fail = vm
        .bind_interface_method::<(i32,), i32>(&player.member(&vm, "fail").unwrap())
        .unwrap();
    assert!(player.call(&mut cx, &fail, (0,)).is_err());
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    let token = CancellationToken::default();
    token.cancel();
    let mut options = vm.execution_options();
    options.cancellation = token;
    assert!(
        player
            .call_with_options(&mut cx, &get, (), options)
            .is_err()
    );
    assert_eq!(player.call(&mut cx, &get, ()).unwrap(), 100);
    let (foreign, foreign_owner) = fixture(SOURCE, None);
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
    let (vm, owner) = fixture(
        r#"
        use std::collections::List;
        pub fn make() -> List<i32> { Vec::from([20, 22]) }
    "#,
        None,
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
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
    let iter = vm
        .bind_interface_method::<(), Interface>(&list.member(&vm, "iter").unwrap())
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(list.call(&mut cx, &len, ()).unwrap(), 2);
    let iterator = list.call(&mut cx, &iter, ()).unwrap();
    let next = vm
        .bind_interface_method::<(), Option<i32>>(&iterator.member(&vm, "next").unwrap())
        .unwrap();
    drop(list);
    vm.collect_garbage().unwrap();
    assert_eq!(iterator.call(&mut cx, &next, ()).unwrap(), Some(20));
    vm.collect_garbage().unwrap();
    assert_eq!(iterator.call(&mut cx, &next, ()).unwrap(), Some(22));
    assert_eq!(iterator.call(&mut cx, &next, ()).unwrap(), None);
    drop(iterator);
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.gc().active_roots(), 0);
}

#[test]
fn interface_calls_preserve_associated_object_outputs_and_public_access() {
    let (vm, owner) = fixture(
        r#"
        pub trait Reader { type Item; fn read(self) -> Self::Item; }
        trait Hidden { fn read(self) -> i32; }
        pub struct Holder<T> { pub val value: T }
        impl<T> Reader for Holder<T> { type Item = T; fn read(self) -> T { self.value } }
        impl Hidden for Holder<i32> { fn read(self) -> i32 { self.value } }
        pub fn make() -> Reader<Item = Holder<i32>> { Holder { value: Holder { value: 42 } } }
        pub fn hidden() -> Hidden { Holder { value: 1 } }
    "#,
        None,
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let value: Interface = vm
        .execute_typed(&_owner_program, "make", (), &Default::default())
        .unwrap();
    let hidden: Interface = vm
        .execute_typed(&_owner_program, "hidden", (), &Default::default())
        .unwrap();
    assert!(hidden.member(&vm, "read").is_err());
    let read = vm
        .bind_interface_method::<(), Object>(&value.member(&vm, "read").unwrap())
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
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
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
    let member = player.member(&vm, "damage").unwrap();
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
    let new_damage = vm
        .bind_interface_method::<(i32,), i32>(&new_player.member(&vm, "damage").unwrap())
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
    assert!(
        !vm.collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    drop(duplicate);
    assert!(
        vm.collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
}
