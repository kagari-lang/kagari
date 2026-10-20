use crate::{
    context::ExecutionContext,
    engine::{KagariEngine, source::PreparationOptions},
    error::EmbeddingError,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;

#[test]
fn prepared_source_survives_session_disposal_and_independent_runtime_binding() {
    let program = {
        let engine = KagariEngine::default();
        engine
            .compile_to_program(
                SourceFile::new("detached.kgr", "fn main() -> i32 { 42 }"),
                PreparationOptions::default(),
            )
            .unwrap()
    };
    #[cfg(feature = "native")]
    assert!(!program.has_native_input());
    for _ in 0..2 {
        // Independent engine installations have independent definition contexts.
        let engine = KagariEngine::default();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .unwrap(),
            Value::I32(42)
        );
    }
}

#[test]
fn cancelled_source_preparation_rejects_both_source_and_checked_input() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(SourceFile::new("cancelled.kgr", "fn main() -> i32 { 42 }"))
        .unwrap();
    let options = PreparationOptions::default();
    options.lowering.cancel.cancel();
    assert!(matches!(
        engine.prepare(&checked, options.clone()),
        Err(EmbeddingError::Cancelled)
    ));
    assert!(matches!(
        engine.compile_to_program(
            SourceFile::new("cancelled.kgr", "fn main() -> i32 { 42 }"),
            options,
        ),
        Err(EmbeddingError::Cancelled)
    ));
}
