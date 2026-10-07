#![cfg(feature = "source")]

use kagari_embed::{
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    native::{function_handle::PinnedFunction, objects::Object},
    session::{ExecutionEvent, ExecutionObserver},
};
use kagari_source::source::SourceFile;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
struct Observer(Arc<AtomicUsize>);

impl ExecutionObserver for Observer {
    fn observe(
        &mut self,
        _: &Runtime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn bound_calls_use_the_sdk_execution_context_and_return_retained_values() {
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://calls.kgr",
                r#"
        pub struct Player { pub var hp: i32 }
        impl Player {
            pub fn damage(self, amount: i32) -> i32 { self.hp = self.hp - amount; self.hp }
        }
        pub fn damage_evidence() -> i32 { make().damage(1) }
        pub fn make() -> Player { Player { hp: 42 } }
        pub fn make_callback() -> fn() -> i32 { val data = [42]; || data[0] }
    "#,
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let make = runtime
        .runtime()
        .bind_function::<(), Object>(&loaded, "make")
        .unwrap();
    let make_callback = runtime
        .runtime()
        .bind_function::<(), PinnedFunction<(), i32>>(&loaded, "make_callback")
        .unwrap();
    let player = runtime.call(&make, (), &context).unwrap();
    let callback = runtime.call(&make_callback, (), &context).unwrap();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.call(&callback, (), &context).unwrap(), 42);
    let hp = runtime
        .runtime()
        .bind_field::<i32>(player.object_type(), "hp")
        .unwrap();
    let damage = runtime
        .runtime()
        .bind_method::<(i32,), i32>(player.object_type(), "damage")
        .unwrap();
    let observations = Arc::new(AtomicUsize::new(0));
    runtime
        .runtime()
        .set_execution_observer(Observer(observations.clone()))
        .unwrap();
    let remaining = runtime
        .with_context(&loaded, &context, |cx| {
            assert_eq!(player.get(cx, &hp)?, 42);
            player.set(cx, &hp, 100)?;
            player.call(cx, &damage, (10,))
        })
        .unwrap();
    assert_eq!(remaining, 90);
    assert!(observations.load(Ordering::Relaxed) > 0);
    let cancelled = ExecutionContext::default();
    cancelled.cancellation.cancel();
    assert!(runtime.call(&make, (), &cancelled).is_err());
    assert!(
        runtime
            .with_context(&loaded, &cancelled, |cx| player.set(cx, &hp, 0))
            .is_err()
    );
    assert_eq!(
        runtime
            .with_context(&loaded, &context, |cx| player.get(cx, &hp))
            .unwrap(),
        90
    );
    let unsupported = ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    };
    assert!(runtime.call(&callback, (), &unsupported).is_err());
    assert_eq!(runtime.call(&callback, (), &context).unwrap(), 42);
    drop(callback);
    drop(player);
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
}
