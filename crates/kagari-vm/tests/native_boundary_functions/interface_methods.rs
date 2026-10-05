use super::fixture;
use crate::compile_program;
use kagari_common::cancellation::CancellationToken;
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, interfaces::Interface, objects::Object,
    registration::FunctionSpec, typed::NativeContext,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const SOURCE: &str = r#"
    pub trait Read<T> { fn get(self) -> T; }
    pub trait Player: Read<i32> {
        fn damage(self, amount: i32) -> i32 { self.get() - amount }
        fn fail(self, divisor: i32) -> i32 { self.get() / divisor }
    }
    struct Data { val hp: i32 }
    impl Read<i32> for Data { fn get(self) -> i32 { self.hp } }
    impl Player for Data {}
    pub fn make(hp: i32) -> Player { Data { hp: hp } }
    pub fn evidence(value: Player) -> i32 { value.damage(2) + value.get() }
"#;

#[test]
fn prepared_interface_members_call_inherited_and_default_methods_on_multiple_receivers() {
    let (vm, owner) = fixture(SOURCE, None);
    let player: Interface = vm.execute_typed(&owner, "make", (100,)).unwrap();
    let second: Interface = vm.execute_typed(&owner, "make", (40,)).unwrap();
    let member = player.member(vm.runtime(), "get").unwrap();
    let direct = player
        .member_declaration(vm.runtime(), member.declaration())
        .unwrap();
    let get = vm
        .runtime()
        .bind_interface_method::<(), i32>(&direct)
        .unwrap();
    assert!(
        vm.runtime()
            .bind_interface_method::<(i32,), i32>(&direct)
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_interface_method::<(), bool>(&direct)
            .is_err()
    );
    let damage = vm
        .runtime()
        .bind_interface_method::<(i32,), i32>(&player.member(vm.runtime(), "damage").unwrap())
        .unwrap();
    assert!(player.member(vm.runtime(), "missing").is_err());
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(player.call(&mut cx, &get, ()).unwrap(), 100);
    assert_eq!(second.call(&mut cx, &get, ()).unwrap(), 40);
    assert_eq!(player.call(&mut cx, &damage, (10,)).unwrap(), 90);
    let fail = vm
        .runtime()
        .bind_interface_method::<(i32,), i32>(&player.member(vm.runtime(), "fail").unwrap())
        .unwrap();
    assert!(player.call(&mut cx, &fail, (0,)).is_err());
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    let token = CancellationToken::default();
    token.cancel();
    let mut options = vm.runtime().execution_options();
    options.cancellation = token;
    assert!(
        player
            .call_with_options(&mut cx, &get, (), options)
            .is_err()
    );
    assert_eq!(player.call(&mut cx, &get, ()).unwrap(), 100);
    let (foreign, foreign_owner) = fixture(SOURCE, None);
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
        pub fn make() -> List<i32> { [20, 22] }
    "#,
        None,
    );
    let list: Interface = vm.execute_typed(&owner, "make", ()).unwrap();
    let registered = StandardDeclarations::default()
        .list()
        .method("len")
        .unwrap();
    let direct = vm
        .runtime()
        .interface_member_declaration(&owner, list.type_argument(), registered.id())
        .unwrap();
    let len = vm
        .runtime()
        .bind_interface_method::<(), usize>(&direct)
        .unwrap();
    let iter = vm
        .runtime()
        .bind_interface_method::<(), Interface>(&list.member(vm.runtime(), "iter").unwrap())
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(list.call(&mut cx, &len, ()).unwrap(), 2);
    let iterator = list.call(&mut cx, &iter, ()).unwrap();
    let next = vm
        .runtime()
        .bind_interface_method::<(), Option<i32>>(&iterator.member(vm.runtime(), "next").unwrap())
        .unwrap();
    drop(list);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(iterator.call(&mut cx, &next, ()).unwrap(), Some(20));
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(iterator.call(&mut cx, &next, ()).unwrap(), Some(22));
    assert_eq!(iterator.call(&mut cx, &next, ()).unwrap(), None);
    drop(iterator);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
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
    let value: Interface = vm.execute_typed(&owner, "make", ()).unwrap();
    let hidden: Interface = vm.execute_typed(&owner, "hidden", ()).unwrap();
    assert!(hidden.member(vm.runtime(), "read").is_err());
    let read = vm
        .runtime()
        .bind_interface_method::<(), Object>(&value.member(vm.runtime(), "read").unwrap())
        .unwrap();
    let result = value
        .call(&mut vm.context(&owner).unwrap(), &read, ())
        .unwrap();
    drop(value);
    drop(hidden);
    vm.runtime().collect_garbage().unwrap();
    let field = vm
        .runtime()
        .bind_field::<i32>(result.object_type(), "value")
        .unwrap();
    assert_eq!(
        result
            .get(&mut vm.context(&owner).unwrap(), &field)
            .unwrap(),
        42
    );
    drop(result);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn retained_interface_calls_cannot_escape_candidate_initialization() {
    let (vm, old) = fixture(SOURCE, None);
    let old_value: Interface = vm.execute_typed(&old, "make", (100,)).unwrap();
    let old_get = vm
        .runtime()
        .bind_interface_method::<(), i32>(&old_value.member(vm.runtime(), "get").unwrap())
        .unwrap();
    let candidate = vm
        .runtime()
        .stage_reload_program(&old, "functions", compile_program(SOURCE, None))
        .unwrap();
    {
        let _session = vm
            .runtime()
            .begin_candidate_initialization(&candidate)
            .unwrap();
        let candidate_value: Interface =
            vm.execute_typed(candidate.module(), "make", (42,)).unwrap();
        let get = vm
            .runtime()
            .bind_interface_method::<(), i32>(&candidate_value.member(vm.runtime(), "get").unwrap())
            .unwrap();
        let mut cx = vm.context(candidate.module()).unwrap();
        assert!(old_value.call(&mut cx, &old_get, ()).is_err());
        assert_eq!(candidate_value.call(&mut cx, &get, ()).unwrap(), 42);
    }
    assert_eq!(
        old_value
            .call(&mut vm.context(&old).unwrap(), &old_get, ())
            .unwrap(),
        100
    );
}

#[test]
fn interface_dispatch_collects_during_reentry_and_pins_old_default_code() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut builder = ModuleBuilder::new(
        "example::interface_methods",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    builder
        .add_function(
            FunctionSpec::new("collect"),
            move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                observed.fetch_add(1, Ordering::Relaxed);
                cx.collect_garbage()?;
                Ok(())
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let source = format!(
        "use example::interface_methods::collect;\n{}",
        SOURCE.replace("self.get() - amount", "collect(); self.get() - amount")
    );
    let (vm, old) = fixture(&source, Some(&module));
    let player: Interface = vm.execute_typed(&old, "make", (100,)).unwrap();
    let member = player.member(vm.runtime(), "damage").unwrap();
    let damage = vm
        .runtime()
        .bind_interface_method::<(i32,), i32>(&member)
        .unwrap();
    let duplicate = vm
        .runtime()
        .bind_interface_method::<(i32,), i32>(&member)
        .unwrap();
    drop(member);
    let new = vm
        .reload_program(
            &old,
            "functions",
            compile_program(
                &source.replace("self.get() - amount", "self.get() - amount - 1"),
                Some(&module),
            ),
        )
        .unwrap();
    let new_player: Interface = vm.execute_typed(&new, "make", (100,)).unwrap();
    let new_damage = vm
        .runtime()
        .bind_interface_method::<(i32,), i32>(&new_player.member(vm.runtime(), "damage").unwrap())
        .unwrap();
    let mut cx = vm.context(&new).unwrap();
    assert!(new_player.call(&mut cx, &damage, (10,)).is_err());
    assert!(player.call(&mut cx, &new_damage, (10,)).is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(player.call(&mut cx, &damage, (10,)).unwrap(), 90);
    assert_eq!(new_player.call(&mut cx, &new_damage, (10,)).unwrap(), 89);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    drop(player);
    drop(damage);
    assert!(
        !vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    drop(duplicate);
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
}
