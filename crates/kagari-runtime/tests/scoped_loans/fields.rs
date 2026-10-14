use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_derive::KagariValue;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    host_scope::scoped::{HostScope, RefMut},
    native::{builder::ModuleBuilder, catalog::DeclarationCatalog, rust_type::RustField},
};
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
    let owner = runtime
        .load_program(
            "field-proof",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let player_type = runtime.bind_rust_type::<Player>(&owner).unwrap();
    let stats_type = runtime.bind_rust_type::<Stats>(&owner).unwrap();
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
            stats.edit(scope, |stats| {
                inventory.edit(scope, |inventory| {
                    stats.hp += 30;
                    inventory.gold -= 5;
                    assert!(whole.read(scope, |_| Ok(())).is_err());
                    assert!(stats_read(&whole, scope, &stats_field).is_err());
                    Ok(())
                })
            })?;
            stats.read(scope, |first| {
                stats.read(scope, |second| {
                    assert_eq!(first.hp, second.hp);
                    assert!(hp.edit(scope, |_| Ok(())).is_err());
                    inventory.edit(scope, |inventory| {
                        inventory.gold += 1;
                        Ok(())
                    })
                })
            })?;
            hp.edit(scope, |value| {
                assert!(stats.edit(scope, |_| Ok(())).is_err());
                assert!(whole.edit(scope, |_| Ok(())).is_err());
                *value += 1;
                Ok(())
            })?;
            whole.edit(scope, |player| {
                player.stats = Stats { hp: 9 };
                Ok(())
            })?;
            hp.read(scope, |hp| {
                assert_eq!(*hp, 9);
                Ok(())
            })?;
            assert!(
                catch_unwind(AssertUnwindSafe(|| hp.edit(
                    scope,
                    |hp| -> Result<(), RuntimeError> {
                        *hp += 1;
                        panic!("projected loan unwind");
                    }
                )))
                .is_err()
            );
            stats.edit(scope, |stats| {
                assert_eq!(stats.hp, 10);
                Ok(())
            })?;
            Ok(())
        })
        .unwrap();
    assert_eq!(player.stats.hp, 10);
    assert_eq!(player.inventory.gold, 21);
}

fn stats_read<'scope>(
    whole: &RefMut<'scope, Player>,
    scope: &HostScope<'scope, '_>,
    field: &RustField<Player, Stats>,
) -> Result<(), RuntimeError> {
    whole.field(scope, field)?.read(scope, |_| Ok(()))
}
