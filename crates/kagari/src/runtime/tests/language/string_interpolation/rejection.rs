use crate::{
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
    test_support::execution,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;

#[test]
fn interpolation_rejects_missing_protocols_and_invalid_formats() {
    for source in [
        r#"struct Item {} fn main() { val item = Item {}; "{item}"; }"#,
        r#"fn render<T>(value: T) -> String { "{value:?}" } fn main() {}"#,
        r#"fn render<T>(value: T) -> String { "{value}" } fn main() {}"#,
        r#"fn main() { val value = 1; "{value:04}"; }"#,
        r#"fn main() { "{missing}"; }"#,
        r#"const NUMBER: i32 = 1; const VALUE: String = "{NUMBER}"; fn main() {}"#,
    ] {
        let result =
            KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
                .unwrap()
                .compile_to_artifact(SourceFile::new("invalid.kgr", source), Default::default());
        assert!(result.is_err(), "{source}");
    }
}

#[test]
fn interpolation_syntax_errors_reach_the_sdk_compile_boundary() {
    let engine = execution::engine(EngineConfig::default());
    let Err(EmbeddingError::Diagnostics { diagnostics }) = engine.compile_to_artifact(
        SourceFile::new(
            "invalid-hole.kgr",
            r#"fn main() { val value = 1; "{value + 1}"; }"#,
        ),
        Default::default(),
    ) else {
        panic!("invalid interpolation must fail source checking");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "KG_TYPE_INVALID_LITERAL"),
        "{diagnostics:?}"
    );
}
