#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    error::RuntimeError,
    native::{builder::ModuleBuilder, catalog::DeclarationCatalog},
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use kagari_derive::KagariValue;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(KagariValue)]
struct Stats {
    hp: i32,
}

#[derive(KagariValue)]
struct Inventory {
    gold: i32,
}

#[derive(KagariValue)]
struct Player {
    stats: Stats,
    inventory: Inventory,
}

#[test]
fn scoped_generated_fields_check_prefix_loans_and_resolve_replacements() {
    let mut builder = ModuleBuilder::new("test::fields", &DeclarationCatalog::default()).unwrap();
    builder.register_type::<Player>().unwrap();
    let module = builder.finish().unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let context = DefinitionContext::new().unwrap();
    let owner_program = load_bytecode(
        &mut runtime,
        "field-proof",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();

    let player_type = runtime
        .bind_rust_type::<Player>(&owner_program.root())
        .unwrap();
    let stats_type = runtime
        .bind_rust_type::<Stats>(&owner_program.root())
        .unwrap();
    let stats_field = runtime
        .bind_rust_field::<Player, Stats>(&player_type, "stats")
        .unwrap();
    let inventory_field = runtime
        .bind_rust_field::<Player, Inventory>(&player_type, "inventory")
        .unwrap();
    let hp_field = runtime
        .bind_rust_field::<Stats, i32>(&stats_type, "hp")
        .unwrap();
    assert!(
        runtime
            .bind_rust_field::<Player, Inventory>(&player_type, "stats")
            .is_err()
    );
    let mut player = Player {
        stats: Stats { hp: 40 },
        inventory: Inventory { gold: 25 },
    };
    runtime
        .with_host_scope(|scope| {
            let whole = scope.borrow_mut(&player_type, &mut player)?;
            let stats = whole.field(scope, &stats_field)?;
            let inventory = whole.field(scope, &inventory_field)?;
            let hp = stats.field(scope, &hp_field)?;
            let stats_view = &stats;
            stats.edit(scope, |scope, stats| {
                inventory.edit(scope, |scope, inventory| {
                    stats.hp += 30;
                    inventory.gold -= 5;
                    assert!(whole.read(scope, |_scope, _| Ok(())).is_err());
                    assert!(stats_view.read(scope, |_scope, _| Ok(())).is_err());
                    Ok(())
                })
            })?;
            stats.read(scope, |scope, first| {
                stats.read(scope, |scope, second| {
                    assert_eq!(first.hp, second.hp);
                    assert!(hp.edit(scope, |_scope, _| Ok(())).is_err());
                    inventory.edit(scope, |_scope, inventory| {
                        inventory.gold += 1;
                        Ok(())
                    })
                })
            })?;
            hp.edit(scope, |scope, value| {
                assert!(stats.edit(scope, |_scope, _| Ok(())).is_err());
                assert!(whole.edit(scope, |_scope, _| Ok(())).is_err());
                *value += 1;
                Ok(())
            })?;
            whole.edit(scope, |_scope, player| {
                player.stats = Stats { hp: 9 };
                Ok(())
            })?;
            hp.read(scope, |_scope, hp| {
                assert_eq!(*hp, 9);
                Ok(())
            })?;
            assert!(
                catch_unwind(AssertUnwindSafe(|| hp.edit(
                    scope,
                    |_scope, hp| -> Result<(), RuntimeError> {
                        *hp += 1;
                        panic!("projected loan unwind");
                    }
                )))
                .is_err()
            );
            stats.edit(scope, |_scope, stats| {
                assert_eq!(stats.hp, 10);
                Ok(())
            })?;
            Ok(())
        })
        .unwrap();
    assert_eq!(player.stats.hp, 10);
    assert_eq!(player.inventory.gold, 21);
}
