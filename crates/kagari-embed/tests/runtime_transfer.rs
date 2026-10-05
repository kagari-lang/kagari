#![cfg(feature = "source")]

use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;
use std::thread;
use tokio::{runtime::Builder, sync::mpsc, task};

#[test]
fn task_owns_a_live_runtime_across_message_receive_awaits() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "transfer.kgr",
                "fn main() -> Vec<i32> { [40] } fn healthy() -> i32 { 42 }",
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let root = runtime
        .execute(&loaded, "main", &[], &context)
        .unwrap()
        .return_value;
    drop(program);
    drop(engine);
    let origin = thread::current().id();
    let executor = Builder::new_multi_thread()
        .worker_threads(2)
        .build()
        .unwrap();
    let (runtime, loaded, root) = executor.block_on(async move {
        let (commands, mut receive) = mpsc::channel::<()>(1);
        let (responses, mut results) = mpsc::channel(1);
        // spawn requires both the future and its returned owned state to be Send.
        let worker = task::spawn(async move {
            while receive.recv().await.is_some() {
                assert_ne!(thread::current().id(), origin);
                runtime.runtime().collect_garbage().unwrap();
                let Value::Array(array) = root.value(runtime.runtime().gc()).unwrap() else {
                    panic!("rooted array");
                };
                let Some(Value::I32(previous)) = runtime.runtime().gc().array_get(array, 0) else {
                    panic!("retained scalar");
                };
                runtime
                    .runtime()
                    .gc()
                    .array_set(array, 0, Value::I32(previous + 1))
                    .unwrap();
                let report = runtime.execute(&loaded, "healthy", &[], &context).unwrap();
                assert_eq!(
                    report
                        .return_value
                        .value(runtime.runtime().gc())
                        .expect("retained execution result"),
                    Value::I32(42)
                );
                assert!(runtime.runtime().execution_root().is_none());
                assert_eq!(
                    runtime.runtime().resources().counters().current_call_depth,
                    0
                );
                responses
                    .send(runtime.runtime().gc().array_get(array, 0).unwrap())
                    .await
                    .unwrap();
            }
            (runtime, loaded, root)
        });
        for expected in [41, 42] {
            commands.send(()).await.unwrap();
            assert_eq!(results.recv().await, Some(Value::I32(expected)));
        }
        drop(commands);
        worker.await.unwrap()
    });
    let Value::Array(array) = root.value(runtime.runtime().gc()).unwrap() else {
        panic!("rooted array");
    };
    assert_eq!(
        runtime.runtime().gc().array_get(array, 0),
        Some(Value::I32(42))
    );
    assert_eq!(
        runtime
            .execute(&loaded, "healthy", &[], &Default::default())
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    drop(root);
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
}
