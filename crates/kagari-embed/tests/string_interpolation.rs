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
fn interpolation_and_join_agree_across_source_artifacts_and_jit() {
    execute(
        r##"
fn generic<T: Display>(value: T) -> String { f"value={value}" }
fn main() -> i32 {
    val name = "world";
    std::debug::assert(f"hello {name}! {1 + 2}" == "hello world! 3", "display");
    std::debug::assert(f"{name:?}" == "\"world\"", "debug");
    std::debug::assert(f"{{{name}}}" == "{world}", "braces");
    std::debug::assert(f"nested: {f"<{name}>"}" == "nested: <world>", "nested");
    std::debug::assert(f"{if true { "yes" } else { "no" }}" == "yes", "expression");
    std::debug::assert(f"你好 \u{1f600} {7}\n" == "你好 😀 7\n", "unicode and escapes");
    std::debug::assert(f"" == "", "empty");
    std::debug::assert("{name}" == "{name}", "ordinary literal");
    std::debug::assert(generic(7) == "value=7", "generic protocol");
    std::debug::assert(["a", "", "b"].join("::") == "a::::b", "join");
    val empty: MutableArray<String> = [];
    std::debug::assert(std::array::join(empty, ",") == "", "empty join");
    42
}
"##,
    );
}

#[test]
fn canonical_protocols_format_each_expression_once_in_order() {
    execute(
        r##"
struct Item { val log: MutableArray<i32>, val id: i32 }
impl Item { fn display(self) -> String { "wrong inherent method" } }
impl Display for Item {
    fn display(self) -> String { self.log.push(self.id); f"{self.id}" }
}
impl Debug for Item {
    fn debug(self) -> String { self.log.push(9); "debug" }
}
fn make(log: MutableArray<i32>, id: i32) -> Item { log.push(0); Item { log, id } }
fn main() -> i32 {
    val log: MutableArray<i32> = [];
    val result = { val std = 7; val Display = 8; f"{make(log, 1)}:{make(log, 2):?}" };
    std::debug::assert(result == "1:debug", "canonical formatting");
    std::debug::assert(log.len() == [0, 0, 0, 0].len(), "exactly once");
    std::debug::assert(log.get("".len_bytes()) == Some(0), "first expression");
    std::debug::assert(log.get([0].len()) == Some(1), "first formatting");
    std::debug::assert(log.get([0, 0].len()) == Some(0), "second expression");
    std::debug::assert(log.get([0, 0, 0].len()) == Some(9), "second formatting");
    42
}
"##,
    );
}

#[test]
fn propagation_and_return_skip_later_parts() {
    execute(
        r##"
fn render(value: Option<i32>, log: MutableArray<i32>) -> Option<String> {
    Some(f"{value?} { { log.push(1); 7 } }")
}
fn early() -> String { f"{ { return "early"; } } {std::debug::panic("unreachable")}" }
fn main() -> i32 {
    val log: MutableArray<i32> = [];
    std::debug::assert(render(None, log) == None, "propagation");
    std::debug::assert(log.is_empty(), "later part skipped");
    std::debug::assert(render(Some(1), log) == Some("1 7"), "normal path");
    std::debug::assert(early() == "early", "return");
    42
}
"##,
    );
}

#[test]
fn interpolation_rejects_missing_protocols_and_invalid_join_types() {
    for source in [
        r#"struct Item {} fn main() { f"{Item {}}"; }"#,
        r#"fn main() { [1, 2].join(","); }"#,
        r#"fn main() { f"{1:04}"; }"#,
        r#"const VALUE: String = f"{1}"; fn main() {}"#,
    ] {
        let result = KagariEngine::default().compile_to_artifact(
            SourceFile::new("invalid.kgr", source),
            Default::default(),
            Default::default(),
        );
        assert!(result.is_err(), "{source}");
    }
}

#[test]
fn formatter_traps_keep_the_origin_and_release_execution_roots() {
    let engine = KagariEngine::default();
    let source = r#"
struct Item { val log: MutableArray<i32> }
impl Display for Item { fn display(self)->String { self.log.push(7); std::debug::panic("format failed"); "" } }
fn main()->String { val log: MutableArray<i32> =[]; f"{Item { log }} {std::debug::panic("later part")}" }
fn healthy()->i32 {42}
"#;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("formatter-trap.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
    assert!(format!("{error:?}").contains("format failed"));
    assert!(error.error_trace().unwrap().frames.len() >= 2);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert!(runtime.runtime().execution_root().is_none());
    assert_eq!(
        runtime
            .execute(&loaded, "healthy", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn formatting_generic_values_uses_their_implementation() {
    execute(
        r#"
struct Item { val value: i32 }
impl Display for Item { fn display(self)->String { f"item={self.value}" } }
fn render<T: Display>(value: T)->String { f"{value}" }
fn main()->i32 {
    std::debug::assert(render(Item { value: 7 }) == "item=7", "generic implementation");
    42
}
"#,
    );
}
