//! Run with `cargo run -p kagari-embed --example async_tasks --no-default-features --features source`.
mod host;
mod provider;

use kagari_embed::engine::KagariEngine;
use kagari_source::source::SourceFile;
use provider::FakeIo;
use std::{env, error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let io = FakeIo::default();
    let mut builder = KagariEngine::builder()?;
    builder.install(io.module("demo::rpc", builder.declarations())?)?;
    builder.install(io.module("demo::database", builder.declarations())?)?;
    let engine = builder.build().map_err(|error| format!("{error:?}"))?;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://async-tasks.kgr", include_str!("script.kgr")),
            Default::default(),
        )
        .map_err(|error| format!("{error:?}"))?;
    // The optional destination emits the exact product for source-free hosts.
    if let Some(destination) = env::args_os().nth(1) {
        let destination = Path::new(&destination);
        if let Some(parent) = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, artifact.to_bytes()?)?;
        println!("Wrote {}", destination.display());
        Ok(())
    } else {
        host::run(engine, artifact, io)
    }
}
