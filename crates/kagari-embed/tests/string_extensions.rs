use crate::BytecodeArtifact;
use crate::ExecutionContext;
use crate::KagariEngine;
use kagari_common::SourceFile;
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("standard-traits.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let mut context = ExecutionContext::default();
        context.capabilities.jit = jit;
        context.language_profile.allow_jit = jit;
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn unicode_queries_and_cleanup_preserve_value_semantics() {
    execute(
        r#"
fn main() -> i32 {
    val original = "　 é😀 x  ";
    std::debug::assert(original.trim() == "é😀 x", "Unicode whitespace");
    std::debug::assert(original.trim_start() == "é😀 x  ", "leading only");
    std::debug::assert(original.trim_end() == "　 é😀 x", "trailing only");
    std::debug::assert(original == "　 é😀 x  ", "unchanged");
    std::debug::assert("　 ".trim() == "", "all whitespace");
    std::debug::assert("".trim() == "", "empty");
    std::debug::assert("é😀é".find("😀") == Some(2usize), "byte offset");
    std::debug::assert("é😀é".rfind("é") == Some(6usize), "last byte offset");
    std::debug::assert("é😀é".find("absent") == None, "missing");
    std::debug::assert("é😀é".rfind("absent") == None, "missing reverse");
    std::debug::assert("é😀é".find("") == Some(0usize), "empty first");
    std::debug::assert("é😀é".rfind("") == Some(8usize), "empty last");
    std::debug::assert("aaaa".rfind("aa") == Some(2usize), "overlap");
    std::debug::assert("é😀é".strip_prefix("é") == Some("😀é"), "prefix once");
    std::debug::assert("é😀é".strip_suffix("é") == Some("é😀"), "suffix once");
    std::debug::assert("abc".strip_prefix("") == Some("abc"), "empty prefix");
    std::debug::assert("abc".strip_suffix("") == Some("abc"), "empty suffix");
    std::debug::assert("abc".strip_prefix("b") == None, "not prefix");
    std::debug::assert("abc".strip_suffix("b") == None, "not suffix");
    std::debug::assert("".strip_prefix("") == Some(""), "empty inputs");
    std::debug::assert(std::string::String::find("éx", "x") == Some(2usize), "qualified");
    42
}
"#,
    );
}

#[test]
fn lazy_splitting_and_line_boundaries() {
    execute(
        r#"
fn main() -> i32 {
    val parts = "a,b,".split(",");
    std::debug::assert(parts.next() == Some("a"), "first");
    std::debug::assert(parts.next() == Some("b"), "second");
    std::debug::assert(parts.next() == Some(""), "trailing");
    std::debug::assert(parts.next() == None, "exhausted");
    std::debug::assert(parts.next() == None, "fused");
    std::debug::assert("é😀".split("").join("|") == "|é|😀|", "scalar boundaries");
    std::debug::assert("".split("").count() == 2usize, "empty delimiter and input");
    std::debug::assert("".split(",").next() == Some(""), "empty field");
    std::debug::assert("a,,b".split(",").join("|") == "a||b", "interior empty");
    std::debug::assert("ababa".split("aba").join("|") == "|ba", "nonoverlapping");
    std::debug::assert("a,b,c".splitn(0usize, ",").next() == None, "zero limit");
    std::debug::assert("a,b,c".splitn(1usize, ",").next() == Some("a,b,c"), "one limit");
    std::debug::assert("a,b,c".splitn(2usize, ",").join("|") == "a|b,c", "remainder");
    std::debug::assert("é😀".splitn(2usize, "").join("|") == "|é😀", "empty bounded");
    std::debug::assert("é😀".splitn(3usize, "").join("|") == "|é|😀", "scalar bounded");
    std::debug::assert(" a　b\t\nc ".split_whitespace().join("|") == "a|b|c", "whitespace");
    std::debug::assert("　 \t\n".split_whitespace().count() == 0usize, "only whitespace");
    std::debug::assert("".lines().count() == 0usize, "no lines");
    std::debug::assert("a\r\nb\nc\r".lines().join("|") == "a|b|c\r", "line endings");
    std::debug::assert("a\n\n".lines().join("|") == "a|", "last newline");
    std::debug::assert("\r\n".lines().next() == Some(""), "empty CRLF line");
    std::debug::assert("a=b=c".split_once("=") == Some(("a", "b=c")), "first delimiter");
    std::debug::assert("a=b=c".rsplit_once("=") == Some(("a=b", "c")), "last delimiter");
    std::debug::assert("abc".split_once("") == Some(("", "abc")), "empty first");
    std::debug::assert("abc".rsplit_once("") == Some(("abc", "")), "empty last");
    std::debug::assert("abc".split_once("=") == None, "absent first");
    std::debug::assert("abc".rsplit_once("=") == None, "absent last");
    std::debug::assert("one two three".split_whitespace().take(2usize).join("+") == "one+two", "pipeline");
    42
}
"#,
    );
}

#[test]
fn transforms_and_byte_iteration() {
    execute(
        r#"
fn main() -> i32 {
    std::debug::assert("aaa".replace("aa", "x") == "xa", "nonoverlapping");
    std::debug::assert("é😀".replace("", "-") == "-é-😀-", "empty pattern boundaries");
    std::debug::assert("aaa".replacen("a", "xx", 2usize) == "xxxxa", "limited replacement");
    std::debug::assert("abc".replacen("", "x", 0usize) == "abc", "zero replacements");
    std::debug::assert("".replace("", "x") == "x", "empty string replacement");
    std::debug::assert("é".repeat(3usize) == "ééé", "repeat");
    std::debug::assert("x".repeat(0usize) == "", "zero repeat");
    std::debug::assert("".repeat(18446744073709551615usize) == "", "empty huge repeat");
    std::debug::assert("".is_ascii(), "empty ASCII");
    std::debug::assert(!"é".is_ascii(), "non ASCII");
    std::debug::assert("Abé".eq_ignore_ascii_case("aBé"), "ASCII folding");
    std::debug::assert(!"É".eq_ignore_ascii_case("é"), "no Unicode folding");
    std::debug::assert("AbÉ".to_ascii_lowercase() == "abÉ", "ASCII lowercase");
    std::debug::assert("Abé".to_ascii_uppercase() == "ABé", "ASCII uppercase");
    std::debug::assert("ΟΣ".to_lowercase() == "ος", "final sigma");
    std::debug::assert("straße".to_uppercase() == "STRASSE", "expansion");
    std::debug::assert("é".bytes().collect::<ArrayList<u8>>()[1usize] == 169u8, "UTF8 bytes");
    val bytes = "😀".bytes();
    std::debug::assert(bytes.next() == Some(240u8), "first byte");
    std::debug::assert(bytes.count() == 3usize, "partial progress");
    std::debug::assert(bytes.next() == None, "fused byte iterator");
    val chars = "é😀x".char_indices();
    std::debug::assert(chars.next() == Some((0usize, "é")), "first scalar");
    std::debug::assert(chars.next() == Some((2usize, "😀")), "second scalar");
    std::debug::assert(chars.next() == Some((6usize, "x")), "third scalar");
    std::debug::assert(chars.next() == None, "end");
    std::debug::assert(chars.next() == None, "fused scalar iterator");
    std::debug::assert("é😀".is_char_boundary(0usize), "start boundary");
    std::debug::assert(!"é😀".is_char_boundary(1usize), "inside scalar");
    std::debug::assert("é😀".is_char_boundary(6usize), "end boundary");
    std::debug::assert(!"é😀".is_char_boundary(7usize), "out of bounds");
    std::debug::assert("".bytes().next() == None, "empty bytes");
    42
}
"#,
    );
}
