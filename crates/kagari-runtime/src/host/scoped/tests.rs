#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::ExecutionRoots,
    native::{builder::ModuleBuilder, catalog::DeclarationCatalog},
    value::view::ValueView,
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
    let owner_program = load_bytecode(
        &mut runtime,
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

    let player_type = runtime
        .bind_rust_type::<Player>(&owner_program.root())
        .unwrap();
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
                |runtime, arguments, destination| {
                    let values = runtime
                        .memory
                        .scope(&runtime.resources, |cx, roots| {
                            ExecutionRoots::with(roots, cx, |slots| {
                                Ok([
                                    arguments.view(slots, 0)?,
                                    arguments.view(slots, 1)?,
                                    arguments.view(slots, 2)?,
                                ])
                            })
                        })
                        .unwrap();
                    let whole = runtime.scoped_view(&values[0])?;
                    let stats = runtime.scoped_view(&values[1])?;
                    let inventory = runtime.scoped_view(&values[2])?;
                    let entered = Cell::new(false);
                    let roots = runtime.gc_stats().root_registrations;
                    let narrowed = {
                        let Runtime {
                            resources,
                            memory,
                            host_borrows,
                            ..
                        } = &mut *runtime;
                        memory
                            .scope(resources, |cx, roots| {
                                ExecutionRoots::with(roots, cx, |slots| {
                                    let value = values[1].get(slots)?;
                                    let narrowed = host_borrows
                                        .narrow_scoped(
                                            slots.scope(),
                                            value,
                                            ReferenceAccess::Shared,
                                        )?
                                        .unwrap()
                                        .unwrap();
                                    slots.protect_local(
                                        values[1].location().unwrap().execution,
                                        narrowed,
                                    )
                                })
                            })
                            .unwrap()
                    };
                    runtime.collect_garbage()?;
                    assert_eq!(runtime.gc_stats().root_registrations, roots);
                    let shared = runtime.scoped_view(&narrowed)?;
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
                    destination.publish(runtime, &ValueView::unit())
                },
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(player.stats.hp, 109);
    assert_eq!(player.inventory.gold, 68);
}
