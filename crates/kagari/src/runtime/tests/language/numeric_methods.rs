use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::Program,
    runtime::tests::language::numeric_checks::NumericChecks,
};
use kagari_runtime::{module::entry::ProgramEntry, value::carriers::RootedValue};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;

mod boundaries;
mod inventory;

const WIDTHS: [(&str, u32); 8] = [
    ("i8", 8),
    ("i16", 16),
    ("i32", 32),
    ("i64", 64),
    ("u8", 8),
    ("u16", 16),
    ("u32", 32),
    ("u64", 64),
];

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::with_native_modules(config, stdlib::modules().unwrap()).unwrap()
}

fn context() -> ExecutionContext {
    ExecutionContext {
        jit_policy: JitPolicy::Disabled,
        ..Default::default()
    }
}

fn compile(source: &str) -> Program {
    // Only serialized executable records cross into the consumer's fresh engine.
    let compiler = engine();
    let artifact = compiler
        .compile_to_artifact(
            SourceFile::new("numeric-methods.kgr", source),
            Default::default(),
        )
        .unwrap_or_else(|error| panic!("{error:?}\n{source}"));
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let decoded = BytecodeArtifact::from_bytes(&bytes).unwrap();
    Program::from_artifact(decoded, &Default::default(), &Default::default()).unwrap()
}

fn execute_checks(checks: Vec<String>) {
    let checks = NumericChecks::new(checks);
    let source = checks.source();
    execute_with(&source, |value| {
        let marker = value.as_i32().unwrap_or_else(|| {
            panic!("encoded: expected numeric check marker, got {value:?}\n{source}")
        });
        checks.assert_result(marker, "encoded", &source);
    });
}

fn execute(source: &str) {
    execute_with(source, |value| {
        assert_eq!(value.as_bool(), Some(true), "{source}")
    });
}

fn execute_with(source: &str, assert_value: impl FnOnce(&RootedValue)) {
    let program = compile(source);
    let context = context();
    let consumer = engine();
    let mut runtime = consumer.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_or_else(|error| panic!("{error:?}\n{source}"));
    assert_value(&result.return_value);
    drop(result);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
}
