#![cfg(feature = "source")]

use kagari_embed::{
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
};
use kagari_runtime::native::{
    function_handle::PinnedFunction, objects::Object, typed::NativeContext,
};
use kagari_source::source::SourceFile;

#[test]
fn bound_calls_use_the_sdk_execution_context_and_return_retained_values() {
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://calls.kgr",
                r#"
        pub struct Player { pub var hp: i32 }
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
    let mut cx = NativeContext::new(runtime.runtime(), &loaded).unwrap();
    assert_eq!(player.get(&mut cx, &hp).unwrap(), 42);
    let cancelled = ExecutionContext::default();
    cancelled.cancellation.cancel();
    assert!(runtime.call(&make, (), &cancelled).is_err());
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
