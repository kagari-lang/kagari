//! Compiler-free host for a trusted bundle built by `kg build-aot`.
use kagari::{
    aot::AotBundle,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
};
use kagari_runtime::{
    host::{HostCallContext, HostError},
    native::builder::ModuleBuilder,
    value::Value,
};
use kagari_types::declaration::logging;
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
    let context = ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    };
    // SAFETY: this example only accepts host-approved immutable bundles produced
    // by the matching Kagari build; it is not a loader for arbitrary uploaded DLLs.
    let bundle = unsafe { AotBundle::load(&path, &context.cancellation) }.expect("trusted bundle");
    // The CLI producer installs host::log. Loading validates all carried native
    // declarations, including services unused by the selected scalar entry.
    let mut engine = KagariEngine::builder().expect("engine registrations");
    let mut logging = ModuleBuilder::from_declaration(
        logging::declaration().expect("logging declaration"),
        engine.declarations(),
        Default::default(),
    )
    .expect("logging module");
    let log = logging.declared_function("log").expect("log function");
    logging
        .bind(log, |cx: &HostCallContext<'_>, args: &[Value]| {
            let [Value::Str(message)] = args else {
                return Err(HostError::new("host.log expects one string argument"));
            };
            let message = cx
                .runtime()
                .gc()
                .string(*message)
                .ok_or_else(|| HostError::new("invalid host.log string"))?;
            println!("{}", &*message);
            Ok(Value::Unit)
        })
        .expect("log binding");
    engine
        .install(logging.finish().expect("complete logging module"))
        .expect("install logging");
    let mut runtime = engine.build().expect("engine").runtime(context.clone());
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
