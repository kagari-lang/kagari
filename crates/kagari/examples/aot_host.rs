//! Compiler-free host for a trusted bundle built by `kg build-aot`.
use kagari::{aot::AotBundle, context::ExecutionContext, engine::KagariEngine};
use kagari_runtime::value::Value;
use kagari_vm::vm::JitExecutionStatus;
use std::{env, path::PathBuf};

fn main() {
    let path = PathBuf::from(
        env::args_os()
            .nth(1)
            .expect("usage: aot_host <trusted-bundle-directory>"),
    )
    .canonicalize()
    .expect("bundle directory");
    let context = ExecutionContext::default();
    // SAFETY: this example only accepts host-approved immutable bundles produced
    // by the matching Kagari build; it is not a loader for arbitrary uploaded DLLs.
    let bundle = unsafe { AotBundle::load(&path, &context.cancellation) }.expect("trusted bundle");
    let mut runtime = KagariEngine::default().runtime(context.clone());
    let loaded = runtime
        .load_program(bundle.program(), Default::default())
        .expect("load program");
    let native = bundle
        .install(&runtime, &loaded)
        .expect("install native entry");
    let entry = bundle.entry();
    let report = runtime
        .execute_prepared(&loaded, entry, &[], &context, &native)
        .expect("execute native code");
    assert_eq!(
        report.jit.as_ref().expect("execution route").status,
        JitExecutionStatus::Native
    );
    assert_eq!(
        report.return_value.value(runtime.runtime().gc()),
        Some(Value::I32(42))
    );
    println!("Native: 42 (compiler-free host)");
}
