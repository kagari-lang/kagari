use crate::{engine::EngineConfig, error::EmbeddingError, test_support::execution};
use kagari_source::source::SourceFile;

#[test]
fn values_interfaces_and_unknown_generic_categories_reject_identity_operators() {
    let engine = execution::engine(EngineConfig::default());
    for source in [
        "fn main() -> bool { 1 === 1 }",
        "fn main() -> bool { \"x\" !== \"x\" }",
        "fn main() -> bool { (1, 2) === (1, 2) }",
        "enum Choice { One } fn main() -> bool { Choice::One === Choice::One }",
        "trait A {} trait B {} fn compare(a: A, b: B) -> bool { a === b }",
        "trait A {} fn compare(a: A, b: A) -> bool { a !== b }",
        "trait A {} struct Item {} impl A for Item {} fn compare(a: A, b: Item) -> bool { a === b }",
        "trait A {} struct Item {} impl A for Item {} fn compare(a: Item, b: A) -> bool { a === b }",
        "fn compare<T>(a: T, b: T) -> bool { a === b }",
        "fn compare(a: fn() -> i32, b: fn() -> i32) -> bool { a === b }",
    ] {
        let Err(EmbeddingError::Diagnostics { diagnostics }) = engine.compile_to_artifact(
            SourceFile::new("invalid-identity.kgr", source),
            Default::default(),
        ) else {
            panic!("identity operands must fail source checking: {source}");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH"),
            "{source}: {diagnostics:?}"
        );
    }
}
