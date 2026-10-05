use super::fixture;
use crate::compile_program;
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, objects::Object, registration::FunctionSpec,
    typed::NativeContext,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const PLAYER: &str = r#"
    pub struct Player { pub var hp: i32 }
    impl Player {
        pub fn create(hp: i32) -> Player { Player { hp: hp } }
        pub fn damage(self, amount: i32) -> i32 { self.hp = self.hp - amount; self.hp }
        pub fn alias(self) -> Player { self }
        pub fn fail(self, divisor: i32) -> i32 { self.hp = self.hp - 1; self.hp / divisor }
        fn hidden(self) -> i32 { self.hp }
        pub(super) fn parent(self) -> i32 { self.hidden() }
    }
    pub struct Enemy { pub var hp: i32 }
    impl Enemy { pub fn damage(self, amount: i32) -> bool { self.hp = self.hp - amount; true } }
    pub fn enemy() -> Enemy { Enemy { hp: 10 } }
    pub fn evidence() -> i32 { val player = Player::create(100); player.damage(2) + player.alias().parent() }
"#;

#[test]
fn prepared_methods_and_associated_functions_enforce_receiver_and_access() {
    let (vm, owner) = fixture(PLAYER, None);
    let runtime = vm.runtime();
    let player_type = runtime.bind_type(&owner, "Player", &[]).unwrap();
    let declaration = player_type.method("damage").unwrap();
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
    let player = vm.call(&create, (100,)).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(player.call(&mut cx, &damage, (10,)).unwrap(), 90);
    let enemy: Object = vm.execute_typed(&owner, "enemy", ()).unwrap();
    assert!(enemy.call(&mut cx, &damage, (10,)).is_err());
    let enemy_hp = runtime
        .bind_field::<i32>(enemy.object_type(), "hp")
        .unwrap();
    assert_eq!(enemy.get(&mut cx, &enemy_hp).unwrap(), 10);
    runtime.collect_garbage().unwrap();
    assert_eq!(player.call(&mut cx, &again, (20,)).unwrap(), 70);
    let alias = runtime
        .bind_method::<(), Object>(&player_type, "alias")
        .unwrap();
    let aliased = player.call(&mut cx, &alias, ()).unwrap();
    let fail = runtime
        .bind_method::<(i32,), i32>(&player_type, "fail")
        .unwrap();
    assert!(player.call(&mut cx, &fail, (0,)).is_err());
    let hp = runtime.bind_field::<i32>(&player_type, "hp").unwrap();
    assert_eq!(aliased.get(&mut cx, &hp).unwrap(), 69);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    let (foreign, foreign_owner) = fixture(PLAYER, None);
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
    );
    builder
        .add_function(
            FunctionSpec::new("collect"),
            move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                cx.collect_garbage()?;
                observed.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        )
        .unwrap();
    let source = PLAYER.replace(
        "pub fn alias(self) -> Player { self }",
        "pub fn alias(self) -> Player { collect(); self }",
    );
    let source = format!("use example::methods::collect;\n{source}");
    let (vm, owner) = fixture(&source, Some(&builder.finish().unwrap()));
    let player_type = vm.runtime().bind_type(&owner, "Player", &[]).unwrap();
    let create = vm
        .runtime()
        .bind_associated_function::<(i32,), Object>(&player_type, "create")
        .unwrap();
    let alias = vm
        .runtime()
        .bind_method::<(), Object>(&player_type, "alias")
        .unwrap();
    let player = vm.call(&create, (42,)).unwrap();
    let retained = player
        .call(&mut vm.context(&owner).unwrap(), &alias, ())
        .unwrap();
    drop(player);
    vm.runtime().collect_garbage().unwrap();
    let hp = vm.runtime().bind_field::<i32>(&player_type, "hp").unwrap();
    assert_eq!(
        retained.get(&mut vm.context(&owner).unwrap(), &hp).unwrap(),
        42
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop(retained);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn prepared_methods_pin_old_code_and_reject_new_generation_receivers() {
    let (vm, old) = fixture(PLAYER, None);
    let old_type = vm.runtime().bind_type(&old, "Player", &[]).unwrap();
    let old_damage = vm
        .runtime()
        .bind_method::<(i32,), i32>(&old_type, "damage")
        .unwrap();
    let old_create = vm
        .runtime()
        .bind_associated_function::<(i32,), Object>(&old_type, "create")
        .unwrap();
    let player = vm.call(&old_create, (100,)).unwrap();
    let source = PLAYER.replace("self.hp - amount", "self.hp - amount - 1");
    let new = vm
        .reload_program(&old, "functions", compile_program(&source, None))
        .unwrap();
    let new_type = vm.runtime().bind_type(&new, "Player", &[]).unwrap();
    let new_damage = vm
        .runtime()
        .bind_method::<(i32,), i32>(&new_type, "damage")
        .unwrap();
    let new_create = vm
        .runtime()
        .bind_associated_function::<(i32,), Object>(&new_type, "create")
        .unwrap();
    let new_player = vm.call(&new_create, (100,)).unwrap();
    let mut cx = vm.context(&new).unwrap();
    assert!(new_player.call(&mut cx, &old_damage, (1,)).is_err());
    assert!(player.call(&mut cx, &new_damage, (1,)).is_err());
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(player.call(&mut cx, &old_damage, (10,)).unwrap(), 90);
    assert_eq!(new_player.call(&mut cx, &new_damage, (10,)).unwrap(), 89);
    let duplicate = vm
        .runtime()
        .bind_method::<(i32,), i32>(&old_type, "damage")
        .unwrap();
    drop(player);
    drop(old_create);
    drop(old_damage);
    drop(old_type);
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
