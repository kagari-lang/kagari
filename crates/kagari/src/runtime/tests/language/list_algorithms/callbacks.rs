use crate::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::{
    gc::roots::RootedValue,
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, views::ValueHandle,
    },
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn cancellation_during_callbacks_restores_storage_and_releases_roots() {
    for (operation, retained_values, preserve_order) in [
        (
            "sort_by(|a,b| { visit(); a.cmp(b) })",
            vec![0, 1, 2, 3],
            false,
        ),
        (
            "retain(|value| { visit(); value != 1 })",
            vec![0, 2, 3],
            false,
        ),
        (
            "filter_map(|value| { visit(); if value == 99 { Some(value) } else { None } }).count()",
            vec![1, 3, 2, 0],
            true,
        ),
    ] {
        let context = ExecutionContext::default();
        let mut module = ModuleBuilder::new(
            "test::cancellation",
            &StandardDeclarations::default()
                .catalog()
                .expect("explicit standard providers"),
        )
        .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let token = context.cancellation.clone();
        let visit = module.define_function(FunctionDecl::new("visit")).unwrap();
        module
            .bind(
                visit,
                move |_cx: &mut CallContext<'_>| -> NativeResult<()> {
                    count.fetch_add(1, Ordering::SeqCst);
                    if count.load(Ordering::SeqCst) == 3 {
                        token.cancel();
                    }
                    Ok(())
                },
            )
            .unwrap();
        let retained: Arc<Mutex<Option<RootedValue>>> = Arc::new(Mutex::new(None));
        let capture = retained.clone();
        let keep = module.define_function(FunctionDecl::new("keep")).unwrap();
        module
            .function(&keep, |function| {
                let parameter = function.type_parameter("T")?.ty();
                function.parameter("value", parameter)?;
                Ok(())
            })
            .unwrap();
        module
            .bind(
                keep,
                move |_cx: &mut CallContext<'_>, value: ValueHandle<'_>| -> NativeResult<()> {
                    *capture.lock().unwrap() = Some(value.root()?);
                    Ok(())
                },
            )
            .unwrap();
        let mut builder = KagariEngine::builder().unwrap();
        builder.install(module.finish().unwrap()).unwrap();
        let engine = builder.build().unwrap();
        let source = format!(
            "use test::cancellation::{{keep, visit}}; fn main() {{ val values = Vec::from([1,3,2,0]); keep(values); values.{operation}; }}"
        );
        let artifact = engine
            .compile_to_artifact(SourceFile::new("cancel.kgr", source), Default::default())
            .unwrap();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap_err()
                .code(),
            "KG_RUNTIME_CANCELLED"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let Value::GcHandle(array) = retained
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .value(runtime.runtime().gc())
            .unwrap()
        else {
            panic!("Vec")
        };
        let mut actual = runtime
            .runtime()
            .gc()
            .sequence_snapshot(array)
            .unwrap()
            .into_iter()
            .map(|value| {
                let Value::I32(value) = value else {
                    panic!("element")
                };
                value
            })
            .collect::<Vec<_>>();
        if !preserve_order {
            actual.sort();
        }
        assert_eq!(actual, retained_values);
        runtime
            .runtime()
            .gc()
            .sequence_push(array, Value::I32(99))
            .unwrap();
        retained.lock().unwrap().take();
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
        assert!(!runtime.runtime().is_quarantined());
    }
}
