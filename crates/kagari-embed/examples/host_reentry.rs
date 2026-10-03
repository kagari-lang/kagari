//! A synchronous host callback invokes the pinned script version and retains its result.
use kagari_contract::{scalar::BuiltinType, types::Ty};
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_vm::reentry::reenter;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    slice,
};

use {kagari_common::host_interface::standard_log, kagari_source::source::SourceFile};

use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    host::{HostError, HostFunction},
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};

#[derive(Debug, Default)]
struct StackDepth(Cell<usize>);

impl ExecutionObserver for StackDepth {
    fn observe(
        &self,
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
                "fn main() -> i32 { print(\"make\"); 42 } fn make() -> ArrayList<i32> { [7, 8] }",
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
    let retained = Rc::new(RefCell::new(None));
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
            *output.borrow_mut() = Some(value);
            Ok(Value::Unit)
        }))
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let session = runtime
        .runtime()
        .begin_execution(&loaded, runtime.runtime().execution_options())
        .unwrap();
    let depth = Rc::new(StackDepth::default());
    runtime
        .runtime()
        .attach_execution_observer(depth.clone())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(depth.0.get(), 2);
    assert_eq!(session.counters().current_call_depth, 0);
    assert_eq!(session.host_scope_count(), 0);
    drop(session);
    runtime.runtime().collect_garbage().unwrap();
    let Value::Array(id) = retained.borrow().as_ref().unwrap().value() else {
        panic!("array result")
    };
    assert_eq!(
        runtime.runtime().gc().array_snapshot(id).unwrap(),
        [Value::I32(7), Value::I32(8)]
    );
    retained.borrow_mut().take();
    runtime.runtime().collect_garbage().unwrap();
    assert!(runtime.runtime().gc().array_snapshot(id).is_none());
    println!("host reentry returned an array; its explicit root survived collection");
}
