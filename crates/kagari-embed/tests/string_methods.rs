#![cfg(feature = "source")]
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;

fn run(source: &str, expected_error: Option<&str>) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(SourceFile::new("strings.kgr", source), Default::default())
        .unwrap();
    let artifact = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime.execute(&loaded, "main", &[], &context);
    match expected_error {
        None => assert_eq!(result.unwrap().return_value, Value::Bool(true)),
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
    val middle = text.slice(1usize, 7usize);
    val replaced = text.replace("é", "E");
    text.len() == 8usize && !text.is_empty() && "".is_empty()
        && text.contains("é🙂") && text.starts_with("aé") && text.ends_with("🙂z")
        && !text.contains("missing") && !text.starts_with("é") && !text.ends_with("🙂")
        && text.find("🙂") == Some(3usize) && text.find("missing") == None
        && middle == "é🙂" && replaced == "aE🙂z" && alias == "aé🙂z" && text == alias
        && text.slice(8usize, 8usize) == "" && "".slice(0usize, 0usize) == ""
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
        && "abc".replace("b", "") == "ac" && "".find("") == Some(0usize)
        && "é🙂".find("") == Some(0usize) && "".contains("") && "".starts_with("") && "".ends_with("")
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
    values.len() == 5usize && values[0] == "" && values[1] == "é" && values[2] == ""
        && values[3] == "🙂" && values[4] == "" && source.text == "|é||🙂|"
        && scalars.len() == 4usize && scalars[0] == "" && scalars[1] == "é" && scalars[2] == "🙂" && scalars[3] == ""
        && combining.len() == 4usize && combining[1] == "e" && combining[2] == "́"
        && empty.len() == 1usize && empty[0] == "" && empty_pattern.len() == 2usize
        && empty_pattern[0] == "" && empty_pattern[1] == "" && ordered.len() == 2usize && ordered[0] == "a"
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
            "fn main() -> bool {{ val value = \"aé🙂z\".slice({start}usize, {end}usize); false }}"
        );
        run(&source, Some("String slice requires ordered byte offsets"));
    }
}

#[test]
fn string_method_signatures_are_statically_checked() {
    let engine = KagariEngine::default();
    for expression in [
        "\"text\".slice(true, 1usize)",
        "\"text\".contains(1)",
        "\"text\".replace(\"x\")",
        "\"text\".split(0)",
    ] {
        let source = format!("fn main() {{ {expression}; }}");
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-string.kgr", source),
                    Default::default()
                )
                .is_err()
        );
    }
}
