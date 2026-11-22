//! Compiler-free host for a trusted bundle built by `kg build-aot`.
use kagari::{
    aot::AotBundle,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
};
use kagari_runtime::{execution::JitExecutionStatus, host::HostError};
#[path = "../tests/support/portable_standard.rs"]
mod portable_standard;
use kagari_contract::native_bundle::BYTECODE_FILE;
use kagari_stdlib::{StandardLibrary, io as script_io, namespaces};
use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
};

fn main() {
    let path = PathBuf::from(
        env::args_os()
            .nth(1)
            .expect("usage: aot_host <trusted-bundle-directory>"),
    )
    .canonicalize()
    .expect("bundle directory");
    let context = ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    };
    // SAFETY: this example only accepts host-approved immutable bundles produced
    // by the matching Kagari build; it is not a loader for arbitrary uploaded DLLs.
    let bundle = unsafe { AotBundle::load(&path, &context.cancellation) }.expect("trusted bundle");
    // The CLI producer installs std::io. Loading validates all carried native
    // declarations, including services unused by the selected scalar entry.
    let mut engine = KagariEngine::builder().expect("engine registrations");
    let library = if engine
        .declarations()
        .authored_origin(&namespaces::module("core", "cmp"))
        .is_some()
    {
        StandardLibrary::new(engine.declarations()).unwrap()
    } else {
        portable_standard::from_bytes(&fs::read(path.join(BYTECODE_FILE)).unwrap()).unwrap()
    };
    engine.install_all(library.modules().unwrap()).unwrap();
    let output = script_io::module(library.declarations().catalog(), |cx, message, suffix| {
        let mut stdout = io::stdout().lock();
        cx.read_string(message, |text| {
            stdout.write_all(text.as_bytes())?;
            stdout.write_all(suffix.as_bytes())?;
            stdout.flush()
        })
        .map_err(|error| HostError::new(error.to_string()))?
        .map_err(|error| HostError::new(error.to_string()))
    })
    .expect("std::io module");
    engine.install(output).expect("install std::io");
    let mut runtime = engine.build().expect("engine").runtime(context.clone());
    let loaded = runtime
        .load_program(bundle.program(), Default::default())
        .expect("load program");
    let native = bundle
        .install(&mut runtime, &loaded)
        .expect("install native entry");
    let entry = bundle.entry();
    let report = runtime
        .execute_prepared(&loaded, entry, &[], &context, &native)
        .expect("execute native code");
    assert_eq!(
        report.jit.as_ref().expect("execution route").status,
        JitExecutionStatus::Native
    );
    assert_eq!(report.return_value.as_i32(), Some(42));
    println!("Native: 42 (compiler-free host)");
}
