//! Explicit trusted native bundle commands; linking stays at the IO boundary.
use crate::CliError;
#[cfg(feature = "aot")]
use crate::{
    print_embedding_error, print_source_error, read_source, runtime_engine, source_engine,
};
#[cfg(feature = "aot")]
use kagari::{
    aot::{AotBundle, AotError},
    context::ExecutionContext,
    program::aot::AotBuild,
};
#[cfg(feature = "aot")]
use kagari_codegen_cranelift::CraneliftBackend;
#[cfg(feature = "aot")]
use kagari_runtime::module::entry::ProgramEntry;
use std::path::Path;
#[cfg(feature = "aot")]
use std::{env, process::Command};

#[cfg(feature = "aot")]
pub(super) fn build(source: &Path, output: &Path) -> Result<(), CliError> {
    let engine = source_engine("target/kagari-declarations")?;
    let artifact = engine
        .compile_to_artifact(read_source(source)?, Default::default())
        .map_err(|error| print_source_error(&engine, error))?;
    let mut backend =
        CraneliftBackend::for_aot().map_err(|e| CliError::message(1, e.to_string()))?;
    let build = AotBuild::compile(
        artifact,
        ProgramEntry::Main,
        &mut backend,
        &Default::default(),
    )
    .map_err(|e| CliError::message(1, e.to_string()))?;
    let linker = env::var_os("KAGARI_AOT_LINKER").unwrap_or_else(|| "lld-link".into());
    build
        .publish(output, &Default::default(), |object, image, symbol| {
            let result = Command::new(&linker)
                .args([
                    "/DLL",
                    "/NOENTRY",
                    "/NODEFAULTLIB",
                    "/MACHINE:X64",
                    "/NOLOGO",
                ])
                .arg(format!("/OUT:{}", image.display()))
                .arg(format!("/EXPORT:{symbol}"))
                .arg(object)
                .output()
                .map_err(|e| {
                    AotError(format!(
                        "cannot run linker {linker:?}: {e}; set KAGARI_AOT_LINKER"
                    ))
                })?;
            if !result.status.success() {
                return Err(AotError(format!(
                    "linker failed ({}): {}{}",
                    result.status,
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                )));
            }
            Ok(())
        })
        .map_err(|e| CliError::message(1, e.to_string()))?;
    println!("built native bundle {}", output.display());
    Ok(())
}

#[cfg(feature = "aot")]
pub(super) fn run(directory: &Path) -> Result<(), CliError> {
    let context = ExecutionContext::default();
    // SAFETY: run-aot is explicitly a trusted-native-code command. Its documented
    // host contract requires an authenticated, immutable deployment directory.
    let bundle = unsafe {
        AotBundle::load(
            &directory
                .canonicalize()
                .map_err(|e| CliError::message(1, e.to_string()))?,
            &context.cancellation,
        )
    }
    .map_err(|e| CliError::message(1, e.to_string()))?;
    let mut runtime = runtime_engine()?.runtime(context.clone());
    let loaded = runtime
        .load_program(bundle.program(), Default::default())
        .map_err(print_embedding_error)?;
    let prepared = bundle
        .install(&runtime, &loaded)
        .map_err(|e| CliError::message(1, e.to_string()))?;
    let report = runtime
        .execute_prepared(&loaded, bundle.entry(), &[], &context, &prepared)
        .map_err(print_embedding_error)?;
    println!(
        "{:?}: {:?}",
        report.jit.as_ref().map(|jit| jit.status),
        report.return_value.value(runtime.runtime().gc())
    );
    if let Some(failure) = report.failure {
        return Err(CliError::message(1, format!("Result::Err: {failure}")));
    }
    Ok(())
}

#[cfg(not(feature = "aot"))]
pub(super) fn build(_: &Path, _: &Path) -> Result<(), CliError> {
    Err(CliError::message(
        2,
        "build-aot requires the aot Cargo feature",
    ))
}

#[cfg(not(feature = "aot"))]
pub(super) fn run(_: &Path) -> Result<(), CliError> {
    Err(CliError::message(
        2,
        "run-aot requires the aot Cargo feature",
    ))
}
