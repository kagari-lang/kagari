#![cfg(feature = "source")]

use kagari::{context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::{native::conversion::context::ConversionContext, value::view::ValueView};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use std::thread;
use tokio::{runtime::Builder, sync::mpsc, task};

#[test]
fn task_owns_a_live_runtime_across_message_receive_awaits() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "transfer.kgr",
                include_str!("../fixtures/embedding/runtime-transfer.kgr"),
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let root = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap()
        .return_value;
    drop(program);
    drop(engine);
    let origin = thread::current().id();
    let executor = Builder::new_multi_thread()
        .worker_threads(2)
        .build()
        .unwrap();
    let (mut runtime, loaded, root) = executor.block_on(async move {
        let (commands, mut receive) = mpsc::channel::<()>(1);
        let (responses, mut results) = mpsc::channel(1);
        // spawn requires both the future and its returned owned state to be Send.
        let worker = task::spawn(async move {
            while receive.recv().await.is_some() {
                assert_ne!(thread::current().id(), origin);
                runtime.collect_garbage().unwrap();
                let updated = {
                    let owner = runtime.select_program(&loaded).unwrap();
                    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
                    let array = cx.import(&root).unwrap();
                    assert_eq!(cx.array_len(&array).unwrap(), 1);
                    let previous = cx
                        .array_element(&array, 0)
                        .unwrap()
                        .unwrap()
                        .as_i32()
                        .unwrap();
                    cx.array_set(&array, 0, &ValueView::from(previous + 1))
                        .unwrap();
                    previous + 1
                };
                let report = runtime.execute(&loaded, "healthy", &[], &context).unwrap();
                assert_eq!(report.return_value.as_i32().unwrap(), 42);
                assert!(runtime.execution_root().is_none());
                assert_eq!(runtime.resources().counters().current_call_depth, 0);
                responses.send(updated).await.unwrap();
            }
            (runtime, loaded, root)
        });
        for expected in [41, 42] {
            commands.send(()).await.unwrap();
            assert_eq!(results.recv().await, Some(expected));
        }
        drop(commands);
        worker.await.unwrap()
    });
    {
        let owner = runtime.select_program(&loaded).unwrap();
        let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
        let array = cx.import(&root).unwrap();
        assert_eq!(
            cx.array_element(&array, 0).unwrap().unwrap().as_i32(),
            Some(42)
        );
    }
    assert_eq!(
        runtime
            .execute(&loaded, "healthy", &[], &Default::default())
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
