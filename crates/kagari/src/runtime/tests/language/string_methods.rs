#![cfg(feature = "source")]
use crate::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;

fn run(source: &str, expected_error: Option<&str>) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(SourceFile::new("strings.kgr", source), Default::default())
        .unwrap();
    let artifact =
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime.execute(&loaded, ProgramEntry::Main, &[], &context);
    match expected_error {
        None => assert_eq!(
            result
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::Bool(true)
        ),
        Some(message) => {
            let error = result.unwrap_err();
            assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
            assert!(format!("{error:?}").contains(message), "{error:?}");
        }
    }
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    // The published version owns its materialized constants. Replace it with an
    // unused version so this assertion still detects leaked execution roots.
    runtime
        .reload_program(&loaded, &program, Default::default())
        .unwrap();
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
    assert!(!runtime.runtime().is_quarantined());
}

#[test]
fn string_methods_use_bytes_and_preserve_immutable_inputs() {
    run(
        r#"
fn main() -> bool {
    val text = "aé🙂z";
    val alias = text;
    val middle = text.slice(1u64, 7u64);
    val replaced = text.replace("é", "E");
    text.len() == 8u64 && !text.is_empty() && "".is_empty()
        && text.contains("é🙂") && text.starts_with("aé") && text.ends_with("🙂z")
        && !text.contains("missing") && !text.starts_with("é") && !text.ends_with("🙂")
        && text.find("🙂") == Some(3u64) && text.find("missing") == None
        && middle == "é🙂" && replaced == "aE🙂z" && alias == "aé🙂z" && text == alias
        && text.slice(8u64, 8u64) == "" && "".slice(0u64, 0u64) == ""
}
"#,
        None,
    );
}

#[test]
fn string_cursors_publish_typed_scalars_byte_offsets_and_utf8_bytes() {
    run(
        r#"
use std::iter::CollectionCursor;
fn source() -> String { "a-é-🙂".replace("-", "") }
fn main() -> bool {
    val chars: CollectionCursor<char> = source().chars();
    val indices: CollectionCursor<(u64, char)> = source().char_indices();
    val bytes: CollectionCursor<u8> = source().bytes();
    val first: Option<char> = chars.next();
    val indexed: Option<(u64, char)> = indices.next();
    val byte: Option<u8> = bytes.next();
    first == Some('a') && chars.next() == Some('é') && chars.next() == Some('🙂')
        && indexed == Some((0u64, 'a')) && indices.next() == Some((1u64, 'é'))
        && indices.next() == Some((3u64, '🙂'))
        && byte == Some(97u8) && bytes.next() == Some(195u8) && bytes.next() == Some(169u8)
        && bytes.next() == Some(240u8) && bytes.next() == Some(159u8)
        && bytes.next() == Some(153u8) && bytes.next() == Some(130u8)
        && chars.next() == None && chars.next() == None
        && indices.next() == None && indices.next() == None
        && bytes.next() == None && bytes.next() == None
        && source().len() == 7u64 && source().chars().count() == 3u64
        && source().char_indices().count() == 3u64 && source().bytes().count() == 7u64
}
"#,
        None,
    );
}

#[test]
fn string_cursors_preserve_nul_combining_marks_and_empty_exhaustion() {
    run(
        r#"
fn main() -> bool {
    val text = "\0e\u{301}";
    val chars = text.chars();
    val indices = text.char_indices();
    val bytes = text.bytes();
    val empty_chars = "".chars();
    val empty_indices = "".char_indices();
    val empty_bytes = "".bytes();
    chars.next() == Some('\0') && chars.next() == Some('e') && chars.next() == Some('\u{301}')
        && chars.next() == None
        && indices.next() == Some((0u64, '\0')) && indices.next() == Some((1u64, 'e'))
        && indices.next() == Some((2u64, '\u{301}')) && indices.next() == None
        && bytes.next() == Some(0u8) && bytes.next() == Some(101u8)
        && bytes.next() == Some(204u8) && bytes.next() == Some(129u8) && bytes.next() == None
        && text.len() == 4u64
        && empty_chars.next() == None && empty_chars.next() == None
        && empty_indices.next() == None && empty_indices.next() == None
        && empty_bytes.next() == None && empty_bytes.next() == None
}
"#,
        None,
    );
}

#[test]
fn trimming_and_literal_replacement_match_unicode_and_empty_patterns() {
    run(
        r#"
fn main() -> bool {
    val spaced = "　 é🙂 \n";
    spaced.trim() == "é🙂" && spaced.trim_start() == "é🙂 \n" && spaced.trim_end() == "　 é🙂"
        && "".trim() == "" && "\t\n　".trim() == ""
        && "aaa".replace("aa", "b") == "ba" && "é🙂".replace("", "-") == "-é-🙂-"
        && "".replace("", "x") == "x" && "abc".replace("missing", "x") == "abc"
        && "abc".replace("b", "") == "ac" && "".find("") == Some(0u64)
        && "é🙂".find("") == Some(0u64) && "".contains("") && "".starts_with("") && "".ends_with("")
}
"#,
        None,
    );
}

#[test]
fn split_is_eager_preserves_empty_fields_and_constructs_a_rooted_list() {
    run(
        r#"use std::collections::{List};

struct Text { val text: String }
fn fields<T: Fn() -> String>(source: T) -> List<String> { source().split("|") }
fn main() -> bool {
    val source = Text { text: "|é||🙂|" };
    val values = fields(|| source.text);
    val scalars = "é🙂".split("");
    val empty = "".split("|");
    val combining = "é".split("");
    val empty_pattern = "".split("");
    val ordered = "b,a,b".split(",").sorted().distinct();
    values.len() == 5u64 && values[0] == "" && values[1] == "é" && values[2] == ""
        && values[3] == "🙂" && values[4] == "" && source.text == "|é||🙂|"
        && scalars.len() == 4u64 && scalars[0] == "" && scalars[1] == "é" && scalars[2] == "🙂" && scalars[3] == ""
        && combining.len() == 4u64 && combining[1] == "e" && combining[2] == "́"
        && empty.len() == 1u64 && empty[0] == "" && empty_pattern.len() == 2u64
        && empty_pattern[0] == "" && empty_pattern[1] == "" && ordered.len() == 2u64 && ordered[0] == "a"
}
"#,
        None,
    );
}

#[test]
fn invalid_string_slices_trap_and_release_execution_state() {
    for (start, end) in [
        (2usize, 3usize),
        (1, 2),
        (7, 3),
        (0, 9),
        (9, 9),
        (0, usize::MAX),
    ] {
        let source = format!(
            "fn main() -> bool {{ val value = \"aé🙂z\".slice({start}u64, {end}u64); false }}"
        );
        let expected = format!(
            "String slice {start}..{end} requires ordered byte offsets within bounds and on UTF-8 boundaries"
        );
        run(&source, Some(&expected));
    }
}

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
