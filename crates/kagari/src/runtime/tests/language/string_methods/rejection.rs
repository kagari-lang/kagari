use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn string_method_signatures_are_statically_checked() {
    let engine = KagariEngine::default();
    for expression in [
        "\"text\".slice(true, 1u64)",
        "\"text\".contains(1)",
        "\"text\".replace(\"x\")",
        "\"text\".split(0)",
        "\"text\".chars(0)",
        "\"text\".char_indices(0)",
        "\"text\".bytes(0)",
        "val wrong: Option<String> = \"é\".chars().next()",
        "val wrong: Option<(u64, String)> = \"é\".char_indices().next()",
        "val wrong: Option<(u32, char)> = \"é\".char_indices().next()",
        "val wrong: Option<u32> = \"é\".bytes().next()",
        "for value in \"text\" {}",
        "\"text\"[0u64]",
        "\"text\"['a']",
        "for value in 'a'..'z' {}",
    ] {
        let source = format!("fn main() {{ {expression}; }}");
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-string.kgr", &source),
                    Default::default()
                )
                .is_err(),
            "unexpectedly accepted: {source}"
        );
    }
}
