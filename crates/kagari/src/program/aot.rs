//! Build a single-entry deployment bundle from checked portable compiler input.
use crate::{
    aot::{AotError, BUILD_CONTRACT, check_cancel, read_bounded},
    program::PreparedProgram,
};
use blake3::hash;
use kagari_abi::{
    native::{ExecutableEntryPoint, NativeLinkDescription},
    version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION},
};
use kagari_bytecode::artifact::{ArtifactBuildOptions, KbcArtifact};
use kagari_codegen::{BackendFunctionInput, object::ObjectBackend};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{map::DefinitionContext, metadata::DefinitionMetadata},
};
use kagari_contract::native_bundle::{
    BYTECODE_FILE, IMAGE_FILE, MANIFEST_FILE, MAX_IMAGE_BYTES, NativeBundleManifest, TARGET,
};
use kagari_mir::ids::InstanceId;
use kagari_runtime::module::entry::ProgramEntry;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

/// Unpublished object and metadata, ready for a platform linker.
/// Publication uses a fresh destination directory and never overwrites a deployment.
#[derive(Debug)]
pub struct AotBuild {
    object: Vec<u8>,
    bytecode: Vec<u8>,
    manifest: NativeBundleManifest,
}

impl AotBuild {
    /// Verify bytecode/MIR correspondence and emit exactly one requested entry.
    /// Unsupported input and any compiler/configuration mismatch are build errors.
    pub fn compile(
        artifact: DefinitionMetadata<KbcArtifact>,
        entry: ProgramEntry<'_>,
        backend: &mut dyn ObjectBackend,
        cancel: &CancellationToken,
    ) -> Result<Self, AotError> {
        check_cancel(cancel)?;
        let program = PreparedProgram::from_artifact(artifact.clone(), &Default::default(), cancel)
            .map_err(|e| AotError(e.to_string()))?;
        let target = match entry {
            ProgramEntry::Main => program.bytecode().entry_function(),
            ProgramEntry::Export(path) => program.bytecode().exported_function(path),
        }
        .ok_or_else(|| AotError("selected AOT entry does not exist".into()))?;
        let mir = program
            .state
            .mir
            .as_ref()
            .ok_or_else(|| AotError("AOT build needs portable MIR".into()))?;
        let links = NativeLinkDescription::scalar();
        let input = BackendFunctionInput::new(
            &mir.modules()[target.module.index()],
            InstanceId::new(target.function.index()),
            &links,
        )
        .ok_or_else(|| AotError("entry missing from MIR".into()))?;
        let configuration = backend.configuration();
        if configuration.target.triple != TARGET
            || configuration.target.pointer_width != 64
            || configuration.backend.as_str() != "cranelift"
            || configuration.target.features != ["sse2"]
        {
            return Err(AotError(
                "AOT requires baseline Windows x64 backend configuration".into(),
            ));
        }
        let product = backend
            .emit_object(input)
            .map_err(|e| AotError(format!("native object compilation: {e:?}")))?;
        let code = &product.artifact.code;
        if backend.configuration() != configuration
            || code.target != configuration.target
            || code.backend != configuration.backend
            || product.artifact.function != target.function
            || code.runtime_abi_version != KAGARI_RUNTIME_ABI_VERSION
            || code.runtime_helper_abi_version != KAGARI_RUNTIME_HELPER_ABI_VERSION
            || product
                .artifact
                .safepoints
                .iter()
                .any(|point| !point.stack_map.live_slots.is_empty())
        {
            return Err(AotError(
                "object product differs from requested contract".into(),
            ));
        }
        let ExecutableEntryPoint::Symbol(symbol) = &code.entry else {
            return Err(AotError("object entry must be symbolic".into()));
        };
        // Rebuild the envelope after dropping compiler input; preserve runtime origins.
        let context = DefinitionContext::new().map_err(|e| AotError(e.to_string()))?;
        let source = artifact
            .import_into(&context, cancel)
            .map_err(|e| AotError(e.to_string()))?
            .into_records();
        let bytecode = KbcArtifact::from_program(
            source.program,
            ArtifactBuildOptions {
                module_epoch: source.header.module_epoch,
                debug: source.debug,
                ..Default::default()
            },
            &context,
            cancel,
        )
        .map_err(|e| AotError(e.to_string()))?
        .to_bytes(&context.snapshot())
        .map_err(|e| AotError(e.to_string()))?;
        let manifest = NativeBundleManifest {
            magic: *b"KGAO",
            format: 1,
            build_contract: BUILD_CONTRACT.into(),
            runtime_abi: KAGARI_RUNTIME_ABI_VERSION.into(),
            helper_abi: KAGARI_RUNTIME_HELPER_ABI_VERSION.into(),
            backend: configuration.backend.to_string(),
            target: configuration.target.triple,
            pointer_width: 64,
            cpu_features: configuration.target.features,
            backend_options: configuration.options,
            bytecode_digest: *hash(&bytecode).as_bytes(),
            image_digest: [0; 32],
            export: match entry {
                ProgramEntry::Main => None,
                ProgramEntry::Export(path) => Some(path.into()),
            },
            module: target.module.index() as u32,
            function: target.function.index() as u32,
            symbol: symbol.clone(),
            safepoints: product
                .artifact
                .safepoints
                .iter()
                .map(|point| point.instruction_offset as u32)
                .collect(),
            overflow_points: code
                .traps
                .iter()
                .map(|point| point.instruction_offset as u32)
                .collect(),
        };
        manifest.validate().map_err(AotError)?;
        check_cancel(cancel)?;
        Ok(Self {
            object: product.bytes,
            bytecode,
            manifest,
        })
    }

    pub fn object(&self) -> &[u8] {
        &self.object
    }

    pub fn symbol(&self) -> &str {
        &self.manifest.symbol
    }

    /// Invoke the supplied linker in a staging directory, then publish atomically.
    /// The destination must be absent; a failed link never replaces existing output.
    /// The callback receives the object path, DLL output path and exported symbol.
    pub fn publish(
        self,
        destination: &Path,
        cancel: &CancellationToken,
        link: impl FnOnce(&Path, &Path, &str) -> Result<(), AotError>,
    ) -> Result<(), AotError> {
        check_cancel(cancel)?;
        if destination.exists() {
            return Err(AotError("bundle destination already exists".into()));
        }
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let parent = parent.canonicalize()?;
        let staging = parent.join(format!(
            ".kagari-aot-{}-{}",
            std::process::id(),
            NEXT_STAGING.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&staging)?;
        let result = (|| {
            let object = staging.join("program.obj");
            let image = staging.join(IMAGE_FILE);
            fs::write(&object, &self.object)?;
            link(&object, &image, self.symbol())?;
            check_cancel(cancel)?;
            let image_bytes = read_bounded(&image, MAX_IMAGE_BYTES)?;
            if image_bytes.is_empty() {
                return Err(AotError("linker emitted an empty image".into()));
            }
            let mut manifest = self.manifest;
            manifest.image_digest = *hash(&image_bytes).as_bytes();
            fs::write(staging.join(BYTECODE_FILE), &self.bytecode)?;
            fs::write(
                staging.join(MANIFEST_FILE),
                manifest.encode().map_err(AotError)?,
            )?;
            fs::remove_file(object)?;
            check_cancel(cancel)?;
            fs::rename(&staging, destination)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }
}

static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);
