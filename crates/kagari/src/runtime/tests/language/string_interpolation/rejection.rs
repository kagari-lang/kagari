use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

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
        let result = KagariEngine::default()
            .compile_to_artifact(SourceFile::new("invalid.kgr", source), Default::default());
        assert!(result.is_err(), "{source}");
    }
}

#[test]
fn interpolation_rejects_expression_holes_and_legacy_prefixes() {
    for literal in [
        r#""{item.value}""#,
        r#""{values[0]}""#,
        r#""{get()}""#,
        r#""{module::VALUE}""#,
        r#""{value + 1}""#,
        r#""{ { val value = 1; value } }""#,
        r#""{ value}""#,
        r#""{value }""#,
        r#""{value :?}""#,
        r#""{value: ?}""#,
        r#""{value:? }""#,
        r#""{}""#,
        r#""{""#,
        r#""}""#,
        r#"f"{value}""#,
        r#"fr"{value}""#,
    ] {
        let source = format!(
            "mod module {{ pub const VALUE: i32 = 1; }} \
             struct Item {{ val value: i32 }} fn get() -> i32 {{ 1 }} \
             fn main() {{ val value = 1; val item = Item {{ value }}; \
             val values = [value]; {literal}; }}"
        );
        let result = KagariEngine::default().compile_to_artifact(
            SourceFile::new("invalid-hole.kgr", &source),
            Default::default(),
        );
        assert!(result.is_err(), "{source}");
    }
}
