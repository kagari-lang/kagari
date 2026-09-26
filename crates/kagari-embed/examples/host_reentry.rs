//! A synchronous host callback invokes the pinned script version and retains its result.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use kagari_common::{SourceFile, host_interface::standard_log};
use kagari_embed::{CompileOptions, ExecutionContext, KagariEngine};
use kagari_runtime::{
    ExecutionEvent, ExecutionFrame, ExecutionObserver, Runtime, RuntimeError,
    host::{HostError, HostFunction},
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
    let mut context = ExecutionContext::default();
    context.language_profile.allow_host_calls = true;
    context.capabilities.host_calls = true;
    context
        .host_policy
        .allowed_host_functions
        .push("host.log".into());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "reentry.kgr",
                "fn main() -> i32 { print(\"make\"); 42 } fn make() -> MutableArray<i32> { [7, 8] }",
            ),
            CompileOptions {
                language_profile: context.language_profile,
            },
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
            let scratch = Value::Array(call.runtime().alloc_array(vec![Value::I32(3)]).unwrap());
            call.retain_temporaries(std::slice::from_ref(&scratch))
                .unwrap();
            let root = call.runtime().execution_root().unwrap();
            let value = kagari_vm::reenter(call, &root, make, &[])
                .map_err(|error| HostError::new(format!("script callback failed: {error:?}")))?;
            call.runtime().collect_garbage().unwrap();
            assert!(call.runtime().gc().validate_value(&scratch));
            *output.borrow_mut() = Some(value);
            Ok(Value::Unit)
        }))
        .unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
