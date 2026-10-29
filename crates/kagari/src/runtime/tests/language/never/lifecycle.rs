use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
    test_support::cancellation as support,
};
use kagari_runtime::{
    host::HostCallContext, module::entry::ProgramEntry, native::builder::ModuleBuilder,
    native::host::HostCallback, value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::declaration::logging;
use std::sync::{Arc, Mutex};

#[test]
fn never_preserves_effects_and_releases_resources_on_traps_and_cancellation() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "never-effects.kgr",
                r#"
fn fail(values: Vec<i32>) -> ! { values.push(20); print("before"); val zero = 0; 1 / zero; loop {} }
fn later(values: Vec<i32>) -> i32 { print("after"); values.push(99); 99 }
fn consume(a: i32, b: i32) -> i32 { a + b }
fn main() -> i32 { val values = Vec::from([1]); consume(fail(values), later(values)) }
pub fn forever() -> ! { loop {} }
fn impossible(value: !) -> i32 { value }
pub fn healthy() -> i32 { 42 }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let captured = effects.clone();
    let mut builder = ModuleBuilder::from_declaration(
        logging::declaration().unwrap(),
        &kagari_stdlib::declarations::StandardDeclarations::default()
            .catalog()
            .unwrap(),
        Default::default(),
    )
    .unwrap();
    let log = builder.declared_function("log").unwrap();
    builder
        .bind(log, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |cx: &HostCallContext<'_>, args: &[Value]| {
                let [Value::Str(id)] = args else {
                    panic!("log string")
                };
                captured
                    .lock()
                    .unwrap()
                    .push(cx.runtime().gc().string_transport(*id).unwrap().to_owned());
                Ok(Value::Unit)
            })
        })
        .unwrap();
    let installed =
        KagariEngine::with_native_modules(Default::default(), vec![builder.finish().unwrap()])
            .unwrap();
    for encoded in [false, true] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext::default();

        let mut runtime = engine.runtime(context.clone());
        let prepared =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        assert!(runtime.load_program(&prepared, Default::default()).is_err());
        effects.lock().unwrap().clear();
        let mut runtime = installed.runtime(context.clone());
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        let error = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
        assert_eq!(*effects.lock().unwrap(), vec!["before"]);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(runtime.runtime().execution_root().is_none());
        let limited = context.clone();

        let cancellation = support::cancel_after(
            runtime.runtime(),
            &runtime.runtime().select_program(&loaded).unwrap(),
            20,
        );
        let error = runtime
            .execute(&loaded, "forever", &[], &limited)
            .unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_CANCELLED");
        drop(cancellation);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(runtime.runtime().execution_root().is_none());

        assert_eq!(
            runtime
                .execute(&loaded, "healthy", &[], &context)
                .unwrap()
                .return_value
                .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}
