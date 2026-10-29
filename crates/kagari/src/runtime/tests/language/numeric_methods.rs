use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
    runtime::tests::language::numeric_checks::NumericChecks,
};
use kagari_runtime::{gc::inspection::ValueView, module::entry::ProgramEntry};
use kagari_source::source::SourceFile;

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
    KagariEngine::new(config)
}

fn context() -> ExecutionContext {
    ExecutionContext {
        jit_policy: JitPolicy::Disabled,
        ..Default::default()
    }
}

fn compile(source: &str) -> PreparedProgram {
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
    PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap()
}

fn execute_checks(checks: Vec<String>) {
    let checks = NumericChecks::new(checks);
    let source = checks.source();
    execute_with(&source, |value| {
        let marker = value.as_i32().unwrap();
        checks.assert_result(marker, "encoded", &source);
    });
}

fn execute(source: &str) {
    execute_with(source, |value| {
        assert!(value.as_bool().unwrap(), "{source}")
    });
}

fn execute_with(source: &str, assert_value: impl FnOnce(ValueView<'_>)) {
    let program = compile(source);
    let context = context();
    let consumer = engine();
    let mut runtime = consumer.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_or_else(|error| panic!("{error:?}\n{source}"));
    result
        .return_value
        .inspect(runtime.runtime(), |view| {
            assert_value(view);
            Ok(())
        })
        .unwrap();
    drop(result);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert!(!runtime.runtime().is_quarantined());
}
