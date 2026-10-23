use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::module::entry::ProgramEntry;
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
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
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
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(
            result
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        drop(result);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn interpolation_executes_across_source_artifacts_and_jit() {
    execute(
        r##"use std::fmt::{Display};
use self::labels::CODE as code;

mod labels { pub const CODE: i32 = 7; }
fn generic<T: Display>(value: T) -> String { "value={value}" }
fn main() -> i32 {
    val name = "world";
    val sum = 1 + 2;
    { val passed = "hello {name}! {sum}" == "hello world! 3"; if !passed {val zero=0;1/zero;} };
    { val passed = "{name:?}" == "\"world\""; if !passed {val zero=0;1/zero;} };
    { val passed = "{{{name}}}" == r"{world}"; if !passed {val zero=0;1/zero;} };
    val nested = "<{name}>";
    { val passed = "nested: {nested}" == "nested: <world>"; if !passed {val zero=0;1/zero;} };
    val choice = if true { "yes" } else { "no" };
    { val passed = "{choice}" == "yes"; if !passed {val zero=0;1/zero;} };
    val number = 7;
    { val passed = "你好 \u{1f600} {number}\n" == "你好 😀 7\n"; if !passed {val zero=0;1/zero;} };
    { val passed = "" == ""; if !passed {val zero=0;1/zero;} };
    { val passed = "{{name}}" == r"{name}"; if !passed {val zero=0;1/zero;} };
    { val passed = generic(7) == "value=7"; if !passed {val zero=0;1/zero;} };
    { val passed = "code={code}" == "code=7"; if !passed {val zero=0;1/zero;} };
    42
}
"##,
    );
}

#[test]
fn raw_and_multiline_values_survive_source_artifacts_and_jit() {
    let source = r##"fn main() -> i32 {
    val name = "world";
    val cooked = "
  {name}
";
    { val passed = cooked == "\n  world\n"; if !passed {val zero=0;1/zero;} };
    val raw = r#"
  {"name":"{name}","path":"\n"}
"#;
    { val passed = raw == "\n  {{\"name\":\"{{name}}\",\"path\":\"\\n\"}}\n"; if !passed {val zero=0;1/zero;} };
    val continued = "left\
        right";
    { val passed = continued == "leftright"; if !passed {val zero=0;1/zero;} };
    val raw_lines = r"left\
        right";
    { val passed = raw_lines == "left\\\n        right"; if !passed {val zero=0;1/zero;} };
    { val passed = "\u{7B}name\u{7D}" == r"{name}"; if !passed {val zero=0;1/zero;} };
    { val passed = "\x41\u{1F_600_}\'\0" == "A😀'\0"; if !passed {val zero=0;1/zero;} };
    match r"{name}" { "{{name}}" => 42, _ => 0 }
}
"##;
    let source = source.replace("\r\n", "\n");
    execute(&source);
    execute(&source.replace('\n', "\r\n"));
}

#[test]
fn interpolation_reads_and_formats_each_occurrence_before_the_next() {
    execute(
        r#"use std::fmt::{Debug, Display};

const INITIAL: i32 = 1;
static var STATE: i32 = INITIAL;
static val LOG: Vec<i32> = Vec::from([]);
static val LATE: i32 = { LOG.push(8); STATE };
struct Item {}
impl Display for Item {
    fn display(self) -> String { STATE += 1; LOG.push(STATE); "display" }
}
impl Debug for Item {
    fn debug(self) -> String { STATE += 10; LOG.push(STATE); "debug" }
}
fn main() -> i32 {
    val item = Item {};
    val result = "{INITIAL}|{STATE}|{item}|{STATE}|{LATE}|{item:?}|{STATE}|{LATE}|{item}";
    { val passed = result == "1|1|display|2|2|debug|12|2|display"; if !passed {val zero=0;1/zero;} };
    { val passed = STATE == 13; if !passed {val zero=0;1/zero;} };
    { val passed = LOG.len() == 4u64; if !passed {val zero=0;1/zero;} };
    { val passed = LOG.get(0u64) == Some(2); if !passed {val zero=0;1/zero;} };
    { val passed = LOG.get(1u64) == Some(8); if !passed {val zero=0;1/zero;} };
    { val passed = LOG.get(2u64) == Some(12); if !passed {val zero=0;1/zero;} };
    { val passed = LOG.get(3u64) == Some(13); if !passed {val zero=0;1/zero;} };
    42
}
"#,
    );
}

#[test]
fn canonical_protocols_format_each_binding_once_in_order() {
    execute(
        r##"use std::fmt::{Debug, Display};

struct Item { val log: Vec<i32>, val id: i32 }
impl Item { fn display(self) -> String { "wrong inherent method" } }
impl Display for Item {
    fn display(self) -> String { self.log.push(self.id); val id = self.id; "{id}" }
}
impl Debug for Item {
    fn debug(self) -> String { self.log.push(9); "debug" }
}
fn make(log: Vec<i32>, id: i32) -> Item { log.push(0); Item { log, id } }
fn main() -> i32 {
    val log: Vec<i32> = Vec::from([]);
    val result = {
        val std = 7;
        val Display = 8;
        val first = make(log, 1);
        val rendered = "{first}";
        val second = make(log, 2);
        "{rendered}:{second:?}"
    };
    { val passed = result == "1:debug"; if !passed {val zero=0;1/zero;} };
    { val passed = log.len() == Vec::from([0, 0, 0, 0]).len(); if !passed {val zero=0;1/zero;} };
    { val passed = log.get(0u64) == Some(0); if !passed {val zero=0;1/zero;} };
    { val passed = log.get(Vec::from([0]).len()) == Some(1); if !passed {val zero=0;1/zero;} };
    { val passed = log.get(Vec::from([0, 0]).len()) == Some(0); if !passed {val zero=0;1/zero;} };
    { val passed = log.get(Vec::from([0, 0, 0]).len()) == Some(9); if !passed {val zero=0;1/zero;} };
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
    val first = value?;
    val rendered = "{first}";
    val second = { log.push(1); 7 };
    Some("{rendered} {second}")
}
fn early(stop: bool) -> String {
    val first = if stop { return "early"; } else { 0 };
    val rendered = "{first}";
    val zero = 0;
    val later = 1 / zero;
    "{rendered} {later}"
}
fn main() -> i32 {
    val log: Vec<i32> = Vec::from([]);
    { val passed = render(None, log) == None; if !passed {val zero=0;1/zero;} };
    { val passed = log.is_empty(); if !passed {val zero=0;1/zero;} };
    { val passed = render(Some(1), log) == Some("1 7"); if !passed {val zero=0;1/zero;} };
    { val passed = early(true) == "early"; if !passed {val zero=0;1/zero;} };
    42
}
"##,
    );
}

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

#[test]
fn formatter_traps_keep_the_origin_and_release_execution_roots() {
    let engine = KagariEngine::default();
    let source = r#"use std::fmt::{Display};

static val LOG: Vec<i32> = Vec::from([]);
static val LATER: i32 = { LOG.push(9); val too_large = 2147483647; too_large + 1 };
struct Item { val log: Vec<i32> }
impl Display for Item { fn display(self)->String { self.log.push(7); val zero=0;1/zero; "" } }
fn main()->String {
    val item = Item { log: LOG };
    "{item} {LATER}"
}
pub fn healthy()->i32 {
    { val passed = LOG.len() == 1u64 && LOG.get(0u64) == Some(7); if !passed {val zero=0;1/zero;} };
    42
}
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
    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
    assert!(format!("{error:?}").contains("division by zero"));
    assert!(error.error_trace().unwrap().frames.len() >= 2);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert!(runtime.runtime().execution_root().is_none());
    assert_eq!(
        runtime
            .execute(&loaded, "healthy", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn formatting_generic_values_uses_their_implementation() {
    execute(
        r#"use std::fmt::{Display};

struct Item { val value: i32 }
impl Display for Item { fn display(self)->String { val value = self.value; "item={value}" } }
fn render<T: Display>(value: T)->String { "{value}" }
fn main()->i32 {
    { val passed = render(Item { value: 7 }) == "item=7"; if !passed {val zero=0;1/zero;} };
    42
}
"#,
    );
}
