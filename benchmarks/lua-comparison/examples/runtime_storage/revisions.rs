//! Separate declarations keep revision controls out of the original setup.
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_source::source::SourceFile;
use std::time::Instant;

use super::sample;

pub(super) fn run(engine: &KagariEngine, context: &ExecutionContext) {
    let start = Instant::now();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("storage_revisions.kgr", include_str!("revisions.kgr")),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    println!(
        "revision_setup,compile_prepare_link_ns={}",
        start.elapsed().as_nanos()
    );
    for name in ["revision_growth", "revision_iterator", "revision_callback"] {
        sample(name, 2_000, || {
            runtime
                .execute_typed(&loaded, name, (2_000_i32,), context)
                .unwrap()
        });
    }
}
