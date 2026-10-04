use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_source::source::SourceFile;

use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("standard-traits.kgr", source),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let result = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn interpolation_executes_across_source_artifacts_and_jit() {
    execute(
        r##"use std::fmt::{Display};

fn generic<T: Display>(value: T) -> String { f"value={value}" }
fn main() -> i32 {
    val name = "world";
    { val passed = f"hello {name}! {1 + 2}" == "hello world! 3"; if !passed {val zero=0;1/zero;} };
    { val passed = f"{name:?}" == "\"world\""; if !passed {val zero=0;1/zero;} };
    { val passed = f"{{{name}}}" == "{world}"; if !passed {val zero=0;1/zero;} };
    { val passed = f"nested: {f"<{name}>"}" == "nested: <world>"; if !passed {val zero=0;1/zero;} };
    { val passed = f"{if true { "yes" } else { "no" }}" == "yes"; if !passed {val zero=0;1/zero;} };
    { val passed = f"你好 \u{1f600} {7}\n" == "你好 😀 7\n"; if !passed {val zero=0;1/zero;} };
    { val passed = f"" == ""; if !passed {val zero=0;1/zero;} };
    { val passed = "{name}" == "{name}"; if !passed {val zero=0;1/zero;} };
    { val passed = generic(7) == "value=7"; if !passed {val zero=0;1/zero;} };
    42
}
"##,
    );
}

#[test]
fn canonical_protocols_format_each_expression_once_in_order() {
    execute(
        r##"use std::fmt::{Debug, Display};

struct Item { val log: Vec<i32>, val id: i32 }
impl Item { fn display(self) -> String { "wrong inherent method" } }
impl Display for Item {
    fn display(self) -> String { self.log.push(self.id); f"{self.id}" }
}
impl Debug for Item {
    fn debug(self) -> String { self.log.push(9); "debug" }
}
fn make(log: Vec<i32>, id: i32) -> Item { log.push(0); Item { log, id } }
fn main() -> i32 {
    val log: Vec<i32> = [];
    val result = { val std = 7; val Display = 8; f"{make(log, 1)}:{make(log, 2):?}" };
    { val passed = result == "1:debug"; if !passed {val zero=0;1/zero;} };
    { val passed = log.len() == [0, 0, 0, 0].len(); if !passed {val zero=0;1/zero;} };
    { val passed = log.get(0usize) == Some(0); if !passed {val zero=0;1/zero;} };
    { val passed = log.get([0].len()) == Some(1); if !passed {val zero=0;1/zero;} };
    { val passed = log.get([0, 0].len()) == Some(0); if !passed {val zero=0;1/zero;} };
    { val passed = log.get([0, 0, 0].len()) == Some(9); if !passed {val zero=0;1/zero;} };
    42
}
"##,
    );
}

#[test]
fn propagation_and_return_skip_later_parts() {
    execute(
        r##"
fn render(value: Option<i32>, log: Vec<i32>) -> Option<String> {
    Some(f"{value?} { { log.push(1); 7 } }")
}
fn early() -> String { f"{ { return "early"; } } {{val zero=0;1/zero}}" }
fn main() -> i32 {
    val log: Vec<i32> = [];
    { val passed = render(None, log) == None; if !passed {val zero=0;1/zero;} };
    { val passed = log.is_empty(); if !passed {val zero=0;1/zero;} };
    { val passed = render(Some(1), log) == Some("1 7"); if !passed {val zero=0;1/zero;} };
    { val passed = early() == "early"; if !passed {val zero=0;1/zero;} };
    42
}
"##,
    );
}

#[test]
fn interpolation_rejects_missing_protocols_and_invalid_formats() {
    for source in [
        r#"struct Item {} fn main() { f"{Item {}}"; }"#,
        r#"fn main() { f"{1:04}"; }"#,
        r#"const VALUE: String = f"{1}"; fn main() {}"#,
    ] {
        let result = KagariEngine::default()
            .compile_to_artifact(SourceFile::new("invalid.kgr", source), Default::default());
        assert!(result.is_err(), "{source}");
    }
}

#[test]
fn formatter_traps_keep_the_origin_and_release_execution_roots() {
    let engine = KagariEngine::default();
    let source = r#"use std::fmt::{Display};

struct Item { val log: Vec<i32> }
impl Display for Item { fn display(self)->String { self.log.push(7); val zero=0;1/zero; "" } }
fn main()->String { val log: Vec<i32> =[]; f"{Item { log }} {{val too_large=2147483647;too_large+1}}" }
fn healthy()->i32 {42}
"#;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("formatter-trap.kgr", source),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
    assert!(format!("{error:?}").contains("division by zero"));
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
        r#"use std::fmt::{Display};

struct Item { val value: i32 }
impl Display for Item { fn display(self)->String { f"item={self.value}" } }
fn render<T: Display>(value: T)->String { f"{value}" }
fn main()->i32 {
    { val passed = render(Item { value: 7 }) == "item=7"; if !passed {val zero=0;1/zero;} };
    42
}
"#,
    );
}
