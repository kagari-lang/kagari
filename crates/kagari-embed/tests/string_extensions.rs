use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
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
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
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
