use super::fixture;
use super::{compile_without_foundation, fixture_without_foundation};
use crate::{declarations::StandardDeclarations, tests::support::bytecode::reload_bytecode};

use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, declarations::FunctionDecl, objects::Object,
    typed::NativeContext,
};

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const PLAYER: &str =
    include_str!("../../../../../tests/fixtures/native/function-player-methods.kgr");

#[test]
fn prepared_methods_and_associated_functions_enforce_receiver_and_access() {
    let (mut vm, owner) = fixture(PLAYER, None);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let runtime = &mut vm;
    let player_type = runtime
        .bind_type(&_owner_program.root(), "Player", &[])
        .unwrap();
    let declaration = player_type.method("damage").unwrap();
    let query = declaration
        .definitions()
        .resolve(declaration.declaration())
        .unwrap()
        .to_path();
    assert_eq!(
        player_type
            .method_declaration(&query)
            .unwrap()
            .declaration(),
        declaration.declaration()
    );
    let damage = runtime
        .bind_method_declaration::<(i32,), i32>(&declaration)
        .unwrap();
    let again = runtime
        .bind_method::<(i32,), i32>(&player_type, "damage")
        .unwrap();
    let create = runtime
        .bind_associated_function::<(i32,), Object>(&player_type, "create")
        .unwrap();
    assert!(
        runtime
            .bind_method::<(), i32>(&player_type, "hidden")
            .is_err()
    );
    assert!(
        runtime
            .bind_method::<(), i32>(&player_type, "parent")
            .is_err()
    );
    assert!(
        runtime
            .bind_method::<(bool,), i32>(&player_type, "damage")
            .is_err()
    );
    assert!(
        runtime
            .bind_method::<(i32,), bool>(&player_type, "damage")
            .is_err()
    );
    assert!(
        runtime
            .bind_method::<(i32,), Object>(&player_type, "create")
            .is_err()
    );
    assert!(
        runtime
            .bind_associated_function::<(Object, i32), i32>(&player_type, "damage")
            .is_err()
    );
    let player = runtime.call(&create, (100,)).unwrap();
    let mut cx = runtime.context(&owner).unwrap();
    assert_eq!(player.call(&mut cx, &damage, (10,)).unwrap(), 90);
    let enemy: Object = cx
        .runtime_mut()
        .execute_typed(&_owner_program, "enemy", (), &Default::default())
        .unwrap();
    assert!(enemy.call(&mut cx, &damage, (10,)).is_err());
    let enemy_hp = cx
        .runtime_mut()
        .bind_field::<i32>(enemy.object_type(), "hp")
        .unwrap();
    assert_eq!(enemy.get(&mut cx, &enemy_hp).unwrap(), 10);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(player.call(&mut cx, &again, (20,)).unwrap(), 70);
    let alias = cx
        .runtime_mut()
        .bind_method::<(), Object>(&player_type, "alias")
        .unwrap();
    let aliased = player.call(&mut cx, &alias, ()).unwrap();
    let fail = cx
        .runtime_mut()
        .bind_method::<(i32,), i32>(&player_type, "fail")
        .unwrap();
    assert!(player.call(&mut cx, &fail, (0,)).is_err());
    let hp = cx
        .runtime_mut()
        .bind_field::<i32>(&player_type, "hp")
        .unwrap();
    assert_eq!(aliased.get(&mut cx, &hp).unwrap(), 69);
    assert_eq!(cx.runtime().resources().counters().current_call_depth, 0);
    let (mut foreign, foreign_owner) = fixture(PLAYER, None);
    let _foreign_owner_program = foreign_owner;
    let foreign_owner = foreign.select_program(&_foreign_owner_program).unwrap();
    assert!(
        player
            .call(&mut foreign.context(&foreign_owner).unwrap(), &damage, (1,))
            .is_err()
    );
    assert_eq!(player.get(&mut cx, &hp).unwrap(), 69);
}

#[test]
fn object_method_reentry_collects_and_retains_returned_aliases() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut builder = ModuleBuilder::new(
        "example::methods",
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
    let source = include_str!("../../../../../tests/fixtures/native/function-player-collect.kgr");
    let (mut vm, owner) = fixture(source, Some(&builder.finish().unwrap()));
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let player_type = vm.bind_type(&_owner_program.root(), "Player", &[]).unwrap();
    let create = vm
        .bind_associated_function::<(i32,), Object>(&player_type, "create")
        .unwrap();
    let alias = vm.bind_method::<(), Object>(&player_type, "alias").unwrap();
    let player = vm.call(&create, (42,)).unwrap();
    let retained = player
        .call(&mut vm.context(&owner).unwrap(), &alias, ())
        .unwrap();
    drop(player);
    vm.collect_garbage().unwrap();
    let hp = vm.bind_field::<i32>(&player_type, "hp").unwrap();
    assert_eq!(
        retained.get(&mut vm.context(&owner).unwrap(), &hp).unwrap(),
        42
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop((retained, player_type, create, alias, hp));
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn prepared_methods_select_current_code_and_preserve_receiver_admission() {
    let (mut vm, old) = fixture_without_foundation(PLAYER, None);
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let old_type = vm.bind_type(&_old_program.root(), "Player", &[]).unwrap();
    let old_damage = vm.bind_method::<(i32,), i32>(&old_type, "damage").unwrap();
    let old_create = vm
        .bind_associated_function::<(i32,), Object>(&old_type, "create")
        .unwrap();
    let player = vm.call(&old_create, (100,)).unwrap();
    assert_eq!(PLAYER.matches("self.hp - amount").count(), 2);
    let source = PLAYER.replace("self.hp - amount", "self.hp - amount - 1");
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(&source, None),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    let new_type = vm.bind_type(&_new_program.root(), "Player", &[]).unwrap();
    let new_damage = vm.bind_method::<(i32,), i32>(&new_type, "damage").unwrap();
    let new_create = vm
        .bind_associated_function::<(i32,), Object>(&new_type, "create")
        .unwrap();
    let new_player = vm.call(&new_create, (100,)).unwrap();
    let mut cx = vm.context(&new).unwrap();
    let enemy: Object = cx
        .runtime_mut()
        .execute_typed(&_new_program, "enemy", (), &Default::default())
        .unwrap();
    assert!(enemy.call(&mut cx, &old_damage, (1,)).is_err());
    assert!(enemy.call(&mut cx, &new_damage, (1,)).is_err());
    drop(enemy);
    assert_eq!(new_player.call(&mut cx, &old_damage, (1,)).unwrap(), 98);
    assert_eq!(player.call(&mut cx, &new_damage, (1,)).unwrap(), 98);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(player.call(&mut cx, &old_damage, (10,)).unwrap(), 87);
    assert_eq!(new_player.call(&mut cx, &new_damage, (10,)).unwrap(), 87);
    let duplicate = cx
        .runtime_mut()
        .bind_method::<(i32,), i32>(&old_type, "damage")
        .unwrap();
    drop(player);
    drop(old_create);
    drop(old_damage);
    drop(old_type);
    cx.runtime_mut().collect_garbage().unwrap();
    assert!(cx.runtime().validate_executable_module(&old).is_ok());
    drop(duplicate);
    cx.runtime_mut().collect_garbage().unwrap();
    assert!(cx.runtime().validate_executable_module(&old).is_err());
}
