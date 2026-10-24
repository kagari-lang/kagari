#![cfg(feature = "source")]
use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
    runtime::Runtime,
};
use kagari_derive::KagariValue;
use kagari_runtime::{
    Runtime as CoreRuntime,
    error::RuntimeError,
    frame::ExecutionFrame,
    native::{
        builder::ModuleBuilder,
        declarations::{FunctionDecl, MethodDecl},
        typed::loans::LoanContext,
        types::Type,
    },
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::reference::ReferenceAccess;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
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
fn scoped_native_calls_accept_compatible_reloaded_descriptors() {
    let mut engine = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("game::reload", engine.declarations()).unwrap();
    module.register_type::<Stats>().unwrap();
    let stats = module.declarations().rust_type::<Stats>().unwrap();
    let read = module
        .define_function(
            FunctionDecl::new("read")
                .parameter("stats", Type::ref_(stats.clone()))
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(read, |_: &mut LoanContext<'_>, (stats,): (&Stats,)| {
            Ok(stats.hp)
        })
        .unwrap();
    module
        .implement(stats, |implementation| {
            implementation.inherent_impl(|methods| {
                let heal = methods.define_method(
                    MethodDecl::instance("heal")
                        .receiver_access(ReferenceAccess::Mutable)
                        .returns(Type::i32()),
                )?;
                methods.bind(heal, |_: &mut LoanContext<'_>, (stats,): (&mut Stats,)| {
                    stats.hp += 1;
                    Ok(stats.hp)
                })
            })
        })
        .unwrap();
    engine.install(module.finish().unwrap()).unwrap();
    let engine = engine.build().unwrap();
    let prepare = |offset| {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "scoped-reload.kgr",
                    format!(
                        r#"
            use game::reload::{{Stats, read}};
            pub fn direct(stats: Ref<Stats>) -> i32 {{ stats.hp }}
            pub fn native(stats: Ref<Stats>) -> i32 {{ read(stats) + {offset} }}
            pub fn heal(stats: RefMut<Stats>) -> i32 {{ stats.heal() }}
        "#
                    ),
                ),
                Default::default(),
            )
            .unwrap();
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
    };
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let old = runtime
        .load_program(&prepare(0), Default::default())
        .unwrap();
    let descriptor = runtime
        .runtime()
        .bind_rust_type::<Stats>(&old.root())
        .unwrap();
    runtime
        .reload_program(&old, &prepare(100), Default::default())
        .unwrap();
    let current = old.clone();
    let mut stats = Stats { hp: 40 };
    runtime
        .with_host_scope(|scope| {
            let view = scope.borrow_mut(&descriptor, &mut stats)?;
            for (loaded, hp) in [(&old, 40), (&current, 41), (&old, 42)] {
                for (entry, expected) in [("direct", hp), ("native", hp + 100), ("heal", hp + 1)] {
                    assert_eq!(
                        Runtime::execute_scoped(scope, loaded, entry, &[view.value()], &context)?,
                        Value::I32(expected)
                    );
                }
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(stats.hp, 43);
    runtime
        .with_host_scope(|scope| {
            let view = scope.borrow(&descriptor, &stats)?;
            assert_eq!(
                Runtime::execute_scoped(scope, &current, "native", &[view.value()], &context)?,
                Value::I32(143)
            );
            assert!(
                Runtime::execute_scoped(scope, &current, "heal", &[view.value()], &context)
                    .is_err()
            );
            Ok(())
        })
        .unwrap();
}

#[derive(Debug)]
struct ContextObserver(Arc<AtomicUsize>);

impl ExecutionObserver for ContextObserver {
    fn observe(
        &mut self,
        runtime: &CoreRuntime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        assert_eq!(runtime.execution_time_millis()?, 123);
        assert!(runtime.execution_options().record_host_calls);
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn scoped_entries_use_explicit_sdk_context_and_the_common_pack_adapter() {
    let mut engine = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("game::scoped", engine.declarations()).unwrap();
    module.register_type::<Player>().unwrap();
    let stats = module.declarations().rust_type::<Stats>().unwrap();
    let inventory = module.declarations().rust_type::<Inventory>().unwrap();
    let heal = module
        .define_function(
            FunctionDecl::new("heal")
                .parameter("stats", Type::ref_mut(stats))
                .parameter("inventory", Type::ref_mut(inventory))
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            heal,
            |cx: &mut LoanContext<'_>, (stats, inventory): (&mut Stats, &mut Inventory)| {
                cx.poll()?;
                stats.hp += 30;
                inventory.gold -= 5;
                Ok(stats.hp)
            },
        )
        .unwrap();
    engine.install(module.finish().unwrap()).unwrap();
    let engine = engine.build().unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "scoped-sdk.kgr",
                r#"
        use game::scoped::{Player, heal};
        pub fn tick(player: RefMut<Player>) -> i32 { heal(player.stats, player.inventory) }
        pub fn inspect(player: Ref<Player>) -> i32 { player.stats.hp }
    "#,
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let player_type = runtime
        .runtime()
        .bind_rust_type::<Player>(&loaded.root())
        .unwrap();
    let observations = Arc::new(AtomicUsize::new(0));
    runtime
        .runtime()
        .set_execution_observer(ContextObserver(observations.clone()))
        .unwrap();
    let mut context = ExecutionContext {
        tracing_enabled: true,
        ..Default::default()
    };
    context.inputs.unix_time_millis = 123;
    let mut player = Player {
        stats: Stats { hp: 40 },
        inventory: Inventory { gold: 25 },
    };
    runtime
        .with_host_scope(|scope| {
            let player = scope.borrow_mut(&player_type, &mut player)?;
            let unsupported = ExecutionContext {
                jit_policy: JitPolicy::Enabled,
                ..context.clone()
            };
            assert!(
                Runtime::execute_scoped(scope, &loaded, "tick", &[player.value()], &unsupported)
                    .is_err()
            );
            let cancelled = context.clone();
            cancelled.cancellation.cancel();
            assert!(
                Runtime::execute_scoped(scope, &loaded, "tick", &[player.value()], &cancelled)
                    .is_err()
            );
            // CancellationToken clones share cancellation. A fresh context establishes
            // a new root; failure cannot leave loans or observer/session state behind.
            let context = ExecutionContext {
                cancellation: Default::default(),
                ..context.clone()
            };
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "inspect", &[player.value()], &context)?,
                Value::I32(40)
            );
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "tick", &[player.value()], &context)?,
                Value::I32(70)
            );
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "inspect", &[player.value()], &context)?,
                Value::I32(70)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(player.stats.hp, 70);
    assert_eq!(player.inventory.gold, 20);
    assert!(observations.load(Ordering::Relaxed) > 0);
}
