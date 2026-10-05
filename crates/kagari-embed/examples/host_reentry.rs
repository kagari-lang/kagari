//! A synchronous host callback invokes the pinned script version and retains its result.
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    host::{HostError, HostFunction},
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::{host_interface::standard_log, scalar::BuiltinType, ty::Ty};
use kagari_vm::reentry::reenter;
use std::sync::{Arc, Mutex};
use std::{cell::Cell, slice};

#[derive(Debug, Default)]
struct StackDepth(Cell<usize>);

impl ExecutionObserver for StackDepth {
    fn observe(
        &mut self,
        _: &Runtime,
        _: ExecutionEvent,
        frames: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        self.0.set(self.0.get().max(frames.len()));
        Ok(())
    }
}

fn main() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "reentry.kgr",
                "fn main() -> i32 { print(\"make\"); 42 } fn make() -> Vec<i32> { [7, 8] }",
            ),
            Default::default(),
        )
        .unwrap();
    // Resolve the entry against this artifact before registering the callback.
    let make = artifact.program.modules[artifact.program.root.index()]
        .functions
        .iter()
        .find(|f| f.name == "make")
        .unwrap()
        .id;
    let retained = Arc::new(Mutex::new(None));
    let output = retained.clone();
    let mut runtime = engine.runtime(context.clone());
    runtime
        .register_host_function(HostFunction::new(standard_log(), move |call, _| {
            let scratch = Value::Array(
                call.runtime()
                    .alloc_array(
                        &call.runtime().execution_root().unwrap(),
                        Ty::Builtin(BuiltinType::I32),
                        vec![Value::I32(3)],
                    )
                    .unwrap(),
            );
            call.retain_temporaries(slice::from_ref(&scratch)).unwrap();
            let root = call.runtime().execution_root().unwrap();
            let value = reenter(call, &root, make, &[])
                .map_err(|error| HostError::new(format!("script callback failed: {error:?}")))?;
            call.runtime().collect_garbage().unwrap();
            assert!(call.runtime().gc().validate_value(&scratch));
            *output.lock().unwrap() = Some(value);
            Ok(Value::Unit)
        }))
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    runtime
        .runtime()
        .set_execution_observer(StackDepth::default())
        .unwrap();
    let session = runtime
        .runtime()
        .begin_execution(&loaded, runtime.runtime().execution_options())
        .unwrap();
    runtime.runtime().attach_execution_observer().unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(
        runtime
            .runtime()
            .execution_observer::<StackDepth>()
            .unwrap()
            .0
            .get(),
        2
    );
    assert_eq!(session.counters().current_call_depth, 0);
    assert_eq!(session.host_scope_count(), 0);
    drop(session);
    runtime.runtime().collect_garbage().unwrap();
    let Value::Array(id) = retained
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .value(runtime.runtime().gc())
        .unwrap()
    else {
        panic!("array result")
    };
    assert_eq!(
        runtime.runtime().gc().array_snapshot(id).unwrap(),
        [Value::I32(7), Value::I32(8)]
    );
    retained.lock().unwrap().take();
    runtime.runtime().collect_garbage().unwrap();
    assert!(runtime.runtime().gc().array_snapshot(id).is_none());
    println!("host reentry returned an array; its explicit root survived collection");
}
