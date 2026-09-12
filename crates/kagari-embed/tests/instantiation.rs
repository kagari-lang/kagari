use kagari_common::SourceFile;
use kagari_embed::{ArtifactOptions, EmbeddingError, KagariEngine};

#[test]
fn instance_limits_report_revision_owned_diagnostics_without_poisoning_compilation() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new(
                "instances.kgr",
                "// 泛型😀\r\nfn echo<T>(value: T) -> T { value } fn main() -> i32 { echo(7) }",
            ),
            Default::default(),
        )
        .unwrap();
    let error = engine
        .emit_bytecode(
            &checked,
            ArtifactOptions {
                lowering: kagari_ir::IrLoweringOptions {
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
        engine
            .source_snapshot()
            .contains(diagnostics[0].span.unwrap())
    );
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    assert_eq!(artifact.module.functions.len(), 2);
}

#[test]
fn cancelled_instantiation_keeps_the_checked_module_reusable() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new("cancel-instances.kgr", "fn main() -> i32 { 7 }"),
            Default::default(),
        )
        .unwrap();
    let options = ArtifactOptions::default();
    options.lowering.cancel.cancel();
    assert!(matches!(
        engine.emit_bytecode(&checked, options),
        Err(EmbeddingError::Cancelled)
    ));
    engine.emit_bytecode(&checked, Default::default()).unwrap();
}

#[test]
fn unresolved_container_inference_is_a_diagnostic_at_codegen() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new("inference.kgr", "fn main() { std::map::new(); }"),
            Default::default(),
        )
        .unwrap();
    let Err(EmbeddingError::Diagnostics { diagnostics }) =
        engine.emit_bytecode(&checked, Default::default())
    else {
        panic!("unresolved type must not enter IR");
    };
    assert_eq!(diagnostics[0].code, "KG_COMPILE_UNRESOLVED_TYPE");
}
