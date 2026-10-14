#![cfg(feature = "source")]
use kagari::{
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
    runtime::KagariRuntime,
};
use kagari_derive::KagariValue;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    native::{
        builder::ModuleBuilder, declarations::FunctionDecl, typed::loans::LoanContext, types::Type,
    },
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_source::source::SourceFile;
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

#[derive(Debug)]
struct ContextObserver(Arc<AtomicUsize>);

impl ExecutionObserver for ContextObserver {
    fn observe(
        &mut self,
        runtime: &Runtime,
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
    let player_type = runtime.runtime().bind_rust_type::<Player>(&loaded).unwrap();
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
                KagariRuntime::execute_scoped(
                    scope,
                    &loaded,
                    "tick",
                    &[player.value()],
                    &unsupported
                )
                .is_err()
            );
            let cancelled = context.clone();
            cancelled.cancellation.cancel();
            assert!(
                KagariRuntime::execute_scoped(
                    scope,
                    &loaded,
                    "tick",
                    &[player.value()],
                    &cancelled
                )
                .is_err()
            );
            // CancellationToken clones share cancellation. A fresh context establishes
            // a new root; failure cannot leave loans or observer/session state behind.
            let context = ExecutionContext {
                cancellation: Default::default(),
                ..context.clone()
            };
            assert_eq!(
                KagariRuntime::execute_scoped(
                    scope,
                    &loaded,
                    "inspect",
                    &[player.value()],
                    &context
                )?,
                Value::I32(40)
            );
            assert_eq!(
                KagariRuntime::execute_scoped(scope, &loaded, "tick", &[player.value()], &context)?,
                Value::I32(70)
            );
            assert_eq!(
                KagariRuntime::execute_scoped(
                    scope,
                    &loaded,
                    "inspect",
                    &[player.value()],
                    &context
                )?,
                Value::I32(70)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(player.stats.hp, 70);
    assert_eq!(player.inventory.gold, 20);
    assert!(observations.load(Ordering::Relaxed) > 0);
}
