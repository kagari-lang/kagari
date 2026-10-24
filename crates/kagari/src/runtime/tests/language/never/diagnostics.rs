use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn rejects_normal_returns_and_nested_never_coercions() {
    let engine = KagariEngine::default();
    for source in [
        "fn wrong() -> ! {}",
        "fn wrong() -> ! { return; }",
        "fn wrong() -> ! { 42 }",
        "fn wrong() -> ! { return 42; }",
        "fn wrong() -> ! { loop { break; } }",
        "fn wrong(x: Vec<!>) -> Vec<i32> { x }",
        "fn wrong(x: Vec<!>) { x[0u64] = 1; }",
        "fn wrong(value: !) { var x: ! = value; x = 1; }",
        "fn wrong(x: fn() -> !) -> fn() -> i32 { x }",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("never-invalid.kgr", source),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}
