//! Compile the actual example once, then execute it with no frontend dependency.
// Cross-target sharing keeps the emitter, standalone consumer and example on the
// exact same public provider and host-driving contract.
#[path = "../../examples/async_tasks/host.rs"]
mod host;
#[path = "../../examples/async_tasks/provider.rs"]
mod provider;

use kagari::{BytecodeArtifact, engine::KagariEngine, program::PreparedProgram};

#[cfg(feature = "source")]
use kagari_source::source::SourceFile;
use provider::FakeIo;
use std::{
    fs,
    future::Future,
    path::Path,
    pin::Pin,
    task::{Context, Poll, Waker},
};

#[test]
fn source_free_structured_pair_contract() {
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("target/fixtures/async_pairs.kbc");
    #[cfg(feature = "source")]
    {
        let source = r#"
use core::task::{join,try_join};
async fn ok(value:i32) -> Result<i32,String> { Ok(value) }
async fn fail() -> Result<i32,String> { Err("original") }
pub async fn run() -> i32 {
    val (a,b)=join(async { 3 },async { 4 }).await;
    val success=try_join(ok(a),ok(b)).await;
    val failure=try_join(ok(99),fail()).await;
    match failure { Err(_) => match success { Ok((x,y)) => x*10+y, Err(_) => 0 }, Ok(_) => 0 }
}
"#;
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://async-pairs.kgr", source),
                Default::default(),
            )
            .unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            artifact.records().to_bytes(artifact.definitions()).unwrap(),
        )
        .unwrap();
    }
    let artifact = BytecodeArtifact::from_bytes(
        &fs::read(path).expect("emit async_pairs with the source feature first"),
    )
    .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut runtime = engine.runtime(Default::default());
    let module = runtime.load_program(&program, Default::default()).unwrap();
    let run = runtime.async_function::<(), i32>(&module, "run").unwrap();
    let mut ticket = runtime.submit_async(&run, ()).unwrap();
    for _ in 0..100 {
        if ticket.is_finished() {
            break;
        }
        assert!(
            runtime
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        runtime.runtime().collect_garbage().unwrap();
    }
    assert!(matches!(
        Pin::new(&mut ticket).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(34))
    ));
    assert!(runtime.take_task_event().is_none());
}

#[test]
fn source_free_async_execution_contract() {
    let io = FakeIo::default();
    let mut builder = KagariEngine::builder().unwrap();
    builder
        .install(io.module("demo::rpc", builder.declarations()).unwrap())
        .unwrap();
    builder
        .install(io.module("demo::database", builder.declarations()).unwrap())
        .unwrap();
    let engine = builder.build().unwrap();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap();
    let path = workspace.join("target/fixtures/async_tasks.kbc");
    #[cfg(feature = "source")]
    {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://async-tasks.kgr",
                    include_str!("../../examples/async_tasks/script.kgr"),
                ),
                Default::default(),
            )
            .unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            artifact.records().to_bytes(artifact.definitions()).unwrap(),
        )
        .unwrap();
    }
    let bytes = fs::read(path).expect("emit the async_tasks example artifact first");
    let artifact = BytecodeArtifact::from_bytes(&bytes).unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    host::run(engine, program, io).unwrap();
}
