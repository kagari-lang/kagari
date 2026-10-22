//! Deployment-specific boundaries; scalar arithmetic remains covered by backend tests.
use super::*;
use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::{
        KagariEngine,
        source::{ArtifactOptions, NativeInputExport},
    },
    program::aot::AotBuild,
};
use bincode::{DefaultOptions, Options};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_runtime::{
    Runtime as CoreRuntime,
    error::RuntimeError,
    frame::ExecutionFrame,
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_vm::vm::JitExecutionStatus;
use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let path = root.join("target").join(format!(
            "aot-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        // Fresh absolute target child created by this test; no user paths are removed.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compile(engine: &KagariEngine, source: &str, entry: ProgramEntry<'_>) -> AotBuild {
    let mut options = ArtifactOptions::default();
    options.lowering.optimization = None;
    let artifact = engine
        .compile_to_artifact(SourceFile::new("deployment.kgr", source), options)
        .unwrap();
    AotBuild::compile(
        artifact,
        entry,
        &mut CraneliftBackend::for_aot().unwrap(),
        &Default::default(),
    )
    .unwrap()
}

fn link(object: &Path, image: &Path, symbol: &str) -> Result<(), AotError> {
    let linker = env::var_os("KAGARI_AOT_LINKER").unwrap_or_else(|| "lld-link".into());
    let result = Command::new(linker)
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
        .output()?;
    if !result.status.success() {
        return Err(AotError(
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ));
    }
    Ok(())
}

fn bundle(engine: &KagariEngine, directory: &Path, source: &str) -> AotBundle {
    compile(engine, source, ProgramEntry::Main)
        .publish(directory, &Default::default(), link)
        .unwrap();
    // SAFETY: freshly generated code, isolated immutable directory, matching runtime.
    unsafe { AotBundle::load(directory, &Default::default()) }.unwrap()
}

fn native_context() -> ExecutionContext {
    ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    }
}

fn result(runtime: &Runtime, loaded: &LoadedModule, prepared: &PreparedNativeEntry, expected: i32) {
    let report = runtime
        .execute_prepared(loaded, ProgramEntry::Main, &[], &native_context(), prepared)
        .unwrap();
    assert_eq!(
        report.jit.as_ref().unwrap().status,
        JitExecutionStatus::Native
    );
    assert_eq!(
        report.return_value.value(runtime.runtime().gc()),
        Some(Value::I32(expected))
    );
}

#[test]
fn installed_images_pin_versions_share_bindings_and_release_after_last_owner() {
    let directory = Directory::new();
    let engine = KagariEngine::default();
    let old_bundle = bundle(
        &engine,
        &directory.0.join("old"),
        "fn main() -> i32 { 40 + 2 }",
    );
    let new_bundle = bundle(
        &engine,
        &directory.0.join("new"),
        "fn main() -> i32 { 40 + 3 }",
    );
    let old_owner = Arc::downgrade(&old_bundle.product.owner);
    let new_owner = Arc::downgrade(&new_bundle.product.owner);
    let mut first = engine.runtime(native_context());
    let mut second = engine.runtime(native_context());
    let old = first
        .load_program(old_bundle.program(), Default::default())
        .unwrap();
    let other = second
        .load_program(old_bundle.program(), Default::default())
        .unwrap();
    let old_native = old_bundle.install(&first, &old).unwrap();
    let other_native = old_bundle.install(&second, &other).unwrap();
    assert!(old_bundle.install(&second, &old).is_err());
    let new = first
        .reload_program(&old, new_bundle.program(), Default::default())
        .unwrap();
    assert!(old_bundle.install(&first, &new).is_err());
    let new_native = new_bundle.install(&first, &new).unwrap();
    let incompatible = engine
        .compile_to_artifact(
            SourceFile::new("deployment.kgr", "fn main() -> bool { true }"),
            Default::default(),
        )
        .unwrap();
    let incompatible =
        PreparedProgram::from_artifact(incompatible, &Default::default(), &Default::default())
            .unwrap();
    assert!(
        first
            .reload_program(&new, &incompatible, Default::default())
            .is_err()
    );
    drop((old_bundle, new_bundle));
    result(&first, &old, &old_native, 42);
    result(&second, &other, &other_native, 42);
    result(&first, &new, &new_native, 43);
    drop(old_native);
    assert!(old_owner.upgrade().is_some());
    drop(other_native);
    assert!(old_owner.upgrade().is_none());
    drop(new_native);
    assert!(new_owner.upgrade().is_none());
    assert_eq!(first.runtime().gc().active_roots(), 0);
    assert_eq!(second.runtime().gc().active_roots(), 0);
}

#[derive(Debug)]
struct Observer;

impl ExecutionObserver for Observer {
    fn observe(
        &mut self,
        _: &CoreRuntime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
}

#[test]
fn deployed_failures_keep_origins_cleanup_and_pre_entry_observer_routing() {
    let directory = Directory::new();
    let engine = KagariEngine::default();
    let overflow = bundle(
        &engine,
        &directory.0.join("overflow"),
        "fn main() -> i32 { 2147483647 + 1 }",
    );
    let mut runtime = engine.runtime(native_context());
    let loaded = runtime
        .load_program(overflow.program(), Default::default())
        .unwrap();
    let prepared = overflow.install(&runtime, &loaded).unwrap();
    let expected_origin = *overflow.manifest.overflow_points.last().unwrap() as usize;
    let bytecode = overflow.program().bytecode();
    let function = bytecode.modules()[bytecode.root().index()]
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let origin = &function.metadata.origins[expected_origin];
    let native_error = runtime
        .execute_prepared(
            &loaded,
            ProgramEntry::Main,
            &[],
            &native_context(),
            &prepared,
        )
        .unwrap_err();
    let interpreted_error = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
        .unwrap_err();
    assert_eq!(native_error.code(), interpreted_error.code());
    assert_eq!(
        native_error.error_trace().unwrap().frames[0].instruction_offset,
        origin.instruction
    );
    assert_eq!(
        native_error.error_trace().unwrap().frames[0].semantic_origin,
        Some(expected_origin)
    );
    assert_eq!(
        interpreted_error.error_trace().unwrap().frames[0].semantic_origin,
        Some(expected_origin)
    );
    assert_eq!(
        native_error.error_trace().unwrap().frames[0].source_span,
        Some(origin.span)
    );
    assert_eq!(
        native_error.error_trace().unwrap().frames[0].source_span,
        interpreted_error.error_trace().unwrap().frames[0].source_span
    );
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);

    let good = bundle(
        &engine,
        &directory.0.join("good"),
        "fn main() -> i32 { 42 }",
    );
    let loaded = runtime
        .load_program(good.program(), Default::default())
        .unwrap();
    let prepared = good.install(&runtime, &loaded).unwrap();
    let context = native_context();
    context.cancellation.cancel();
    assert!(
        runtime
            .execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
            .is_err()
    );
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    result(&runtime, &loaded, &prepared, 42);
    runtime
        .runtime_mut()
        .set_execution_observer(Observer)
        .unwrap();
    let session = runtime
        .runtime()
        .begin_execution(&loaded, runtime.runtime().execution_options())
        .unwrap();
    runtime.runtime().attach_execution_observer().unwrap();
    let report = runtime
        .execute_prepared(
            &loaded,
            ProgramEntry::Main,
            &[],
            &native_context(),
            &prepared,
        )
        .unwrap();
    assert_eq!(
        report.jit.as_ref().unwrap().status,
        JitExecutionStatus::InterpreterFallback
    );
    assert_eq!(
        report.return_value.value(runtime.runtime().gc()),
        Some(Value::I32(42))
    );
    drop(report);
    drop(session);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn malformed_deployments_are_rejected_before_loading_native_code() {
    let directory = Directory::new();
    let engine = KagariEngine::default();
    let path = directory.0.join("input");
    let original = bundle(&engine, &path, "fn main() -> i32 { 42 }");
    let original_manifest = original.manifest.clone();
    drop(original);
    for case in 0..12 {
        let mut manifest = original_manifest.clone();
        match case {
            0 => manifest.build_contract = "0".repeat(64),
            1 => manifest.runtime_abi = "stale".into(),
            2 => manifest.helper_abi = "stale".into(),
            3 => manifest.target = "aarch64-pc-windows-msvc".into(),
            4 => manifest.cpu_features.push("avx512f".into()),
            5 => manifest.bytecode_digest[0] ^= 1,
            6 => manifest.image_digest[0] ^= 1,
            7 => manifest.export = Some("missing".into()),
            8 => manifest.module += 1,
            9 => manifest.symbol = "missing".into(),
            10 => manifest.safepoints.push(0),
            _ => {
                manifest.function += 1;
                manifest.symbol = format!("kagari_native_{}", manifest.function);
            }
        }
        // Bypass the writer's validation deliberately to test the reader boundary.
        let bytes = DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .serialize(&manifest)
            .unwrap();
        fs::write(path.join(MANIFEST_FILE), bytes).unwrap();
        assert!(
            unsafe { AotBundle::load(&path, &Default::default()) }.is_err(),
            "case {case}"
        );
    }
    let encoded = original_manifest.encode().unwrap();
    assert!(NativeBundleManifest::decode(&encoded[..encoded.len() - 1], BUILD_CONTRACT).is_err());
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(NativeBundleManifest::decode(&trailing, BUILD_CONTRACT).is_err());
    fs::write(path.join(MANIFEST_FILE), encoded).unwrap();
    let image = fs::read(path.join(IMAGE_FILE)).unwrap();
    fs::write(path.join(IMAGE_FILE), &image[..image.len() - 1]).unwrap();
    assert!(unsafe { AotBundle::load(&path, &Default::default()) }.is_err());
    fs::write(path.join(IMAGE_FILE), image).unwrap();
    assert!(unsafe { AotBundle::load(Path::new("relative"), &Default::default()) }.is_err());
    assert!(unsafe { AotBundle::load(&path, &Default::default()) }.is_ok());
    let bytecode_only = engine
        .compile_to_artifact(
            SourceFile::new("deployment.kgr", "fn main() -> i64 { 42i64 }"),
            ArtifactOptions {
                native_input: NativeInputExport::BytecodeOnly,
                ..Default::default()
            },
        )
        .unwrap();
    let bytes = bytecode_only
        .records()
        .to_bytes(bytecode_only.definitions())
        .unwrap();
    let mut wrong_signature = original_manifest;
    wrong_signature.bytecode_digest = *hash(&bytes).as_bytes();
    fs::write(path.join(BYTECODE_FILE), bytes).unwrap();
    fs::write(path.join(MANIFEST_FILE), wrong_signature.encode().unwrap()).unwrap();
    let error = unsafe { AotBundle::load(&path, &Default::default()) }.unwrap_err();
    assert!(error.0.contains("signature/points"), "{error}");
}

#[test]
fn publication_is_complete_and_build_errors_preserve_existing_deployments() {
    let directory = Directory::new();
    let engine = KagariEngine::default();
    let host_specific = engine
        .compile_to_artifact(
            SourceFile::new("host.kgr", "fn main() -> i32 { 42 }"),
            Default::default(),
        )
        .unwrap();
    let error = AotBuild::compile(
        host_specific,
        ProgramEntry::Main,
        &mut CraneliftBackend::for_host().unwrap(),
        &Default::default(),
    )
    .unwrap_err();
    assert!(error.0.contains("baseline"), "{error}");
    let failed = directory.0.join("failed");
    let build = compile(&engine, "fn main() -> i32 { 42 }", ProgramEntry::Main);
    assert!(
        build
            .publish(&failed, &Default::default(), |_, image, _| {
                fs::write(image, b"incomplete")?;
                Err(AotError("link failed".into()))
            })
            .is_err()
    );
    assert!(!failed.exists());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
    let destination = directory.0.join("export");
    compile(
        &engine,
        "mod implementation { pub fn answer() -> i32 { 42 } } pub use crate::implementation::answer;",
        ProgramEntry::Export("answer"),
    )
    .publish(&destination, &Default::default(), link)
    .unwrap();
    let manifest = fs::read(destination.join(MANIFEST_FILE)).unwrap();
    let build = compile(
        &engine,
        "mod implementation { pub fn answer() -> i32 { 43 } } pub use crate::implementation::answer;",
        ProgramEntry::Export("answer"),
    );
    assert!(
        build
            .publish(&destination, &Default::default(), |_, _, _| panic!(
                "must not link over deployment"
            ))
            .is_err()
    );
    assert_eq!(manifest, fs::read(destination.join(MANIFEST_FILE)).unwrap());
    let loaded = unsafe { AotBundle::load(&destination, &Default::default()) }.unwrap();
    let mut runtime = engine.runtime(native_context());
    let module = runtime
        .load_program(loaded.program(), Default::default())
        .unwrap();
    let prepared = loaded.install(&runtime, &module).unwrap();
    let report = runtime
        .execute_prepared(&module, loaded.entry(), &[], &native_context(), &prepared)
        .unwrap();
    assert_eq!(
        report.jit.as_ref().unwrap().status,
        JitExecutionStatus::Native
    );
    assert_eq!(
        report.return_value.value(runtime.runtime().gc()),
        Some(Value::I32(42))
    );
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "unsupported.kgr",
                "fn main() -> i32 { var x = 1; x += 2; x }",
            ),
            Default::default(),
        )
        .unwrap();
    assert!(
        AotBuild::compile(
            artifact,
            ProgramEntry::Main,
            &mut CraneliftBackend::for_aot().unwrap(),
            &Default::default()
        )
        .is_err()
    );
}
