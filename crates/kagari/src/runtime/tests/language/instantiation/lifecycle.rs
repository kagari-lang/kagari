use crate::{
    engine::{KagariEngine, source::ArtifactOptions},
    error::EmbeddingError,
};
use kagari_compiler::source::lower::instances::MirLoweringOptions;
use kagari_source::source::SourceFile;

#[test]
fn instance_limits_report_revision_owned_diagnostics_without_poisoning_compilation() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(SourceFile::new(
            "instances.kgr",
            "// 泛型😀\r\nfn echo<T>(value: T) -> T { value } fn main() -> i32 { echo(7) }",
        ))
        .unwrap();
    let error = engine
        .emit_artifact(
            &checked,
            ArtifactOptions {
                lowering: MirLoweringOptions {
                    max_generic_instances: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("structured diagnostic");
    };
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "KG_COMPILE_LIMIT_EXCEEDED");
    assert!(
        checked.program().modules().iter().any(|module| {
            let span = diagnostics[0].span.unwrap();
            module.lowered.source.span(span.range) == Some(span)
        }),
        "{diagnostics:?}"
    );
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    assert_eq!(
        artifact.records().program.modules[artifact.records().program.root.index()]
            .functions
            .len(),
        2
    );
}

#[test]
fn cancelled_instantiation_keeps_the_checked_module_reusable() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(SourceFile::new(
            "cancel-instances.kgr",
            "fn main() -> i32 { 7 }",
        ))
        .unwrap();
    let options = ArtifactOptions::default();
    options.lowering.cancel.cancel();
    assert!(matches!(
        engine.emit_artifact(&checked, options),
        Err(EmbeddingError::Cancelled)
    ));
    engine.emit_artifact(&checked, Default::default()).unwrap();
}
