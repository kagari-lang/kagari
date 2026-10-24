//! A synchronous host callback invokes the pinned script version and retains its result.
use kagari::{context::ExecutionContext, engine::KagariEngine};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    host::{HostCallContext, HostError},
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_vm::reentry::reenter;
use std::{
    cell::Cell,
    slice,
    sync::{Arc, Mutex},
};

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
    let mut builder = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("example::reentry", builder.declarations()).unwrap();
    let notify = module
        .define_function(
            FunctionDecl::new("notify")
                .parameter("message", Type::scalar(BuiltinType::String))
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        may_call_host_services: true,
                        may_trap: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    let retained = Arc::new(Mutex::new(None));
    let output = retained.clone();
    module
        .bind(notify, move |call: &HostCallContext<'_>, _: &[Value]| {
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
            // Resolve only in the current pinned program, never through source
            // analysis or a newer globally published generation.
            let make = root
                .bytecode
                .functions
                .iter()
                .find(|function| function.name == "make")
                .ok_or_else(|| HostError::new("missing pinned make entry"))?
                .id;
            let value = reenter(call, &root, make, &[])
                .map_err(|error| HostError::new(format!("script callback failed: {error:?}")))?;
            call.runtime().collect_garbage().unwrap();
            assert!(call.runtime().gc().validate_value(&scratch));
            *output.lock().unwrap() = Some(value);
            Ok(Value::Unit)
        })
        .unwrap();
    builder.install(module.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let context = ExecutionContext::default();
    let program = engine.compile_to_program(
        SourceFile::new(
            "reentry.kgr",
            "use example::reentry::notify; fn main() -> i32 { notify(\"make\"); 42 } fn make() -> [i32] { [7, 8] }",
        ),
        Default::default(),
    ).unwrap();
    let mut runtime = engine.runtime(context.clone());

    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let _selected_loaded = runtime.runtime().select_program(&loaded).unwrap();
    runtime
        .runtime()
        .set_execution_observer(StackDepth::default())
        .unwrap();
    let session = runtime
        .runtime()
        .begin_execution(&_selected_loaded, runtime.runtime().execution_options())
        .unwrap();
    runtime.runtime().attach_execution_observer().unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
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
