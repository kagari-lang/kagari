use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    error::RuntimeError,
    native::{builder::ModuleBuilder, catalog::DeclarationCatalog},
    value::Value,
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use kagari_derive::KagariValue;
use kagari_types::reference::ReferenceAccess;
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
};

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
fn atomic_generated_path_packs_release_conflicts_errors_and_unwinds() {
    let mut builder = ModuleBuilder::new("test::packs", &DeclarationCatalog::default()).unwrap();
    builder.register_type::<Player>().unwrap();
    let module = builder.finish().unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let context = DefinitionContext::new().unwrap();
    let owner = runtime
        .load_program(
            "pack-proof",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
    let player_type = runtime.bind_rust_type::<Player>(&owner).unwrap();
    let stats_field = runtime
        .bind_rust_field::<Player, Stats>(&player_type, "stats")
        .unwrap();
    let inventory_field = runtime
        .bind_rust_field::<Player, Inventory>(&player_type, "inventory")
        .unwrap();
    let mut player = Player {
        stats: Stats { hp: 40 },
        inventory: Inventory { gold: 25 },
    };
    runtime
        .with_host_scope(|scope| {
            let whole = scope.borrow_mut(&player_type, &mut player)?;
            let stats = whole.field(scope, &stats_field)?;
            let inventory = whole.field(scope, &inventory_field)?;
            scope.execute(
                &[whole.value(), stats.value(), inventory.value()],
                |runtime, values| {
                    let whole = runtime.scoped_view(&values[0])?;
                    let stats = runtime.scoped_view(&values[1])?;
                    let inventory = runtime.scoped_view(&values[2])?;
                    let entered = Cell::new(false);
                    let narrowed = runtime
                        .narrow_scoped(&values[1], ReferenceAccess::Shared)?
                        .unwrap();
                    let roots = runtime.gc().root_execution_values(vec![narrowed]).unwrap();
                    runtime.collect_garbage()?;
                    let shared = runtime.scoped_view(&roots.get(runtime.gc(), 0).unwrap())?;
                    assert!(
                        runtime
                            .host_borrows
                            .edit_scoped::<Stats, _>(runtime, shared, |_| {
                                entered.set(true);
                                Ok(())
                            })
                            .is_err()
                    );
                    assert!(!entered.get());
                    runtime
                        .host_borrows
                        .read_scoped::<Stats, _>(runtime, shared, |stats| {
                            assert_eq!(stats.hp, 40);
                            Ok(())
                        })?;
                    assert!(
                        runtime
                            .host_borrows
                            .edit_pair::<Stats, Stats, _>(runtime, stats, stats, |_, _| {
                                entered.set(true);
                                Ok(())
                            })
                            .is_err()
                    );
                    assert!(!entered.get());
                    // The first acquisition in the rejected pack must already be gone.
                    runtime
                        .host_borrows
                        .edit_scoped::<Stats, _>(runtime, stats, |stats| {
                            stats.hp += 1;
                            Ok(())
                        })?;
                    assert!(
                        runtime
                            .host_borrows
                            .edit_read_pair::<Player, Stats, _>(runtime, whole, stats, |_, _| {
                                entered.set(true);
                                Ok(())
                            })
                            .is_err()
                    );
                    assert!(!entered.get());
                    runtime.host_borrows.read_pair::<Stats, Stats, _>(
                        runtime,
                        stats,
                        stats,
                        |a, b| {
                            assert_eq!(a.hp, b.hp);
                            Ok(())
                        },
                    )?;
                    runtime.host_borrows.read_edit_pair::<Stats, Inventory, _>(
                        runtime,
                        stats,
                        inventory,
                        |stats, inventory| {
                            inventory.gold += stats.hp;
                            Ok(())
                        },
                    )?;
                    runtime.host_borrows.edit_read_pair::<Stats, Inventory, _>(
                        runtime,
                        stats,
                        inventory,
                        |stats, inventory| {
                            stats.hp += inventory.gold;
                            Ok(())
                        },
                    )?;
                    assert!(
                        runtime
                            .host_borrows
                            .edit_pair::<Stats, Inventory, ()>(
                                runtime,
                                stats,
                                inventory,
                                |stats, inventory| {
                                    stats.hp += 1;
                                    inventory.gold += 1;
                                    Err(RuntimeError::host_call_failure("completed pair writes"))
                                }
                            )
                            .is_err()
                    );
                    assert!(
                        catch_unwind(AssertUnwindSafe(|| runtime
                            .host_borrows
                            .edit_pair::<Stats, Inventory, ()>(
                                runtime,
                                stats,
                                inventory,
                                |stats, inventory| {
                                    stats.hp += 1;
                                    inventory.gold += 1;
                                    panic!("pair unwind");
                                }
                            )))
                        .is_err()
                    );
                    runtime.host_borrows.edit_pair::<Stats, Inventory, _>(
                        runtime,
                        stats,
                        inventory,
                        |stats, inventory| {
                            assert_eq!(stats.hp, 109);
                            assert_eq!(inventory.gold, 68);
                            Ok(())
                        },
                    )?;
                    Ok(Value::Unit)
                },
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(player.stats.hp, 109);
    assert_eq!(player.inventory.gold, 68);
}
