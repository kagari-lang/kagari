//! A synchronous host callback invokes the pinned script version and retains its result.
use std::{cell::RefCell, rc::Rc};

use kagari_common::{SourceFile, host_interface::standard_log};
use kagari_embed::{CompileOptions, ExecutionContext, KagariEngine};
use kagari_runtime::{
    host::{HostError, HostFunction},
    value::Value,
};

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
                "fn main() -> i32 { print(\"make\"); 42 } fn make() -> [i32] { [7, 8] }",
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
            let root = call.runtime().execution_root().unwrap();
            let value = kagari_vm::reenter(call, &root, make, &[])
                .map_err(|error| HostError::new(format!("script callback failed: {error:?}")))?;
            call.runtime().collect_garbage().unwrap();
            *output.borrow_mut() = Some(value);
            Ok(Value::Unit)
        }))
        .unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
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
