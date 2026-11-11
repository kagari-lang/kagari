use crate::{
    context::ExecutionContext,
    engine::{KagariEngine, source::PreparationOptions},
    error::EmbeddingError,
    program::{Program, ProgramPreparationError},
};
use kagari_runtime::{Runtime as CoreRuntime, module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;

#[test]
fn prepared_source_survives_session_disposal_and_independent_runtime_binding() {
    fn send_sync<T: Send + Sync>() {}

    send_sync::<Program>();
    let program = {
        let engine =
            KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
                .unwrap();
        engine
            .compile_to_program(
                SourceFile::new("detached.kgr", "fn main() -> i32 { 42 }"),
                PreparationOptions::default(),
            )
            .unwrap()
    };
    // The same canonical product is directly installable without an SDK Runtime.
    let mut direct = CoreRuntime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &stdlib::modules().unwrap(),
        &mut direct,
    )
    .unwrap();
    let installed = direct
        .load_verified_program("direct", program.clone().into_verified())
        .unwrap();
    let vm = direct;
    let loaded = vm.select_program(&installed).unwrap();
    let result = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    assert_eq!(result.return_value.value(vm.gc()).unwrap(), Value::I32(42));
    drop(result);
    assert_eq!(vm.gc().active_roots(), 0);
    for _ in 0..2 {
        // Independent engine installations have independent definition contexts.
        let engine =
            KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.gc())
                .unwrap(),
            Value::I32(42)
        );
    }
}

#[test]
fn cancelled_preparation_rejects_source_checked_and_artifact_input() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let checked = engine
        .compile_source(SourceFile::new("cancelled.kgr", "fn main() -> i32 { 42 }"))
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let options = PreparationOptions::default();
    options.lowering.cancel.cancel();
    assert!(matches!(
        Program::from_artifact(artifact, &Default::default(), &options.lowering.cancel),
        Err(ProgramPreparationError::Cancelled)
    ));
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
