//! Compile the actual example once, then execute it with no frontend dependency.
// Cross-target sharing keeps the emitter, standalone consumer and example on the
// exact same public provider and host-driving contract.
#[path = "../../examples/async_tasks/host.rs"]
mod host;
#[path = "../../examples/async_tasks/provider.rs"]
mod provider;

use kagari_embed::{BytecodeArtifact, engine::KagariEngine};
#[cfg(feature = "source")]
use kagari_source::source::SourceFile;
use provider::FakeIo;
use std::{fs, path::Path};

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
        fs::write(&path, artifact.to_bytes().unwrap()).unwrap();
    }
    let bytes = fs::read(path).expect("emit the async_tasks example artifact first");
    let artifact = BytecodeArtifact::from_bytes(&bytes).unwrap();
    host::run(engine, artifact, io).unwrap();
}
