use crate::{engine::KagariEngine, error::EmbeddingError};
use kagari_source::source::SourceFile;

#[test]
fn generic_trait_method_bounds_reject_invalid_arguments() {
    let source = include_str!("../../../../../../../examples/generic-trait-methods.kgr")
        .replace("fn echo<U>(", "fn echo<U: Eq + Hash>(")
        .replace("fn echo<V>(", "fn echo<V: Eq + Hash>(")
        .replace("value.echo(42)", "value.echo(1.5); 42");
    let error = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new("generic-method-bound.kgr", source),
            Default::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected source diagnostics")
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED" }),
        "{diagnostics:?}"
    );
}

#[test]
fn unresolved_container_inference_is_a_diagnostic_before_codegen() {
    let engine = KagariEngine::default();
    let Err(EmbeddingError::Diagnostics { diagnostics }) = engine.compile_source(SourceFile::new(
        "inference.kgr",
        "use std::collections::{HashMap};\nfn main() { HashMap::new(); }",
    )) else {
        panic!("unresolved type must not enter checked HIR");
    };
    assert_eq!(diagnostics[0].code, "KG_TYPE_CANNOT_INFER_GENERIC_ARGUMENT");
}
