//! Explicit loading of trusted native bundles without compiler dependencies.
use crate::{program::PreparedProgram, runtime::KagariRuntime};
use blake3::hash;
use kagari_abi::{
    native::NativeCodeOwner, native_call::JitCompiledFunction, representation::ValueType,
};
use kagari_bytecode::artifact::{KbcArtifact, MAX_ARTIFACT_BYTES};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::{
    native::NativeCompilationProduct,
    native_bundle::{
        BYTECODE_FILE, IMAGE_FILE, MANIFEST_FILE, MAX_IMAGE_BYTES, MAX_MANIFEST_BYTES,
        NativeBundleManifest,
    },
};
use kagari_runtime::module::{LoadedModule, entry::ProgramEntry};
use kagari_vm::vm::native::PreparedNativeEntry;
use libloading::Library;
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Arc,
};

pub(crate) const BUILD_CONTRACT: &str = env!("KAGARI_NATIVE_BUILD_CONTRACT");

#[derive(Debug, thiserror::Error)]
#[error("AOT: {0}")]
pub struct AotError(pub String);

impl From<io::Error> for AotError {
    fn from(error: io::Error) -> Self {
        Self(error.to_string())
    }
}

#[derive(Debug)]
struct LibraryOwner {
    _library: Library,
}

impl NativeCodeOwner for LibraryOwner {}

/// A verified bytecode Program and trusted loaded image, independent of any runtime.
/// Clones and installed entries retain the same immutable library owner.
#[derive(Debug, Clone)]
pub struct AotBundle {
    manifest: NativeBundleManifest,
    program: PreparedProgram,
    product: Arc<NativeCompilationProduct>,
}

impl AotBundle {
    /// Load a native bundle from an absolute immutable deployment directory.
    /// Rejects malformed/mismatched bytecode, image digests, targets and contracts.
    ///
    /// # Safety
    /// The caller must trust the producer's machine code and all image dependencies,
    /// including loader initialization/destruction, to obey Kagari's native ABI and
    /// exact function semantics without unwinding. The directory and dependencies
    /// must remain immutable through validation, loading and all retained uses;
    /// other actors must not replace files or redirect paths. Hashes are not proof
    /// of provenance or code correctness. Loading a DLL can execute initialization.
    pub unsafe fn load(directory: &Path, cancel: &CancellationToken) -> Result<Self, AotError> {
        check_cancel(cancel)?;
        if !directory.is_absolute() {
            return Err(AotError("bundle path must be absolute".into()));
        }
        if !cfg!(all(
            target_arch = "x86_64",
            target_os = "windows",
            target_env = "msvc"
        )) {
            return Err(AotError(
                "AOT loading currently requires x86_64 Windows MSVC".into(),
            ));
        }
        let directory = directory.canonicalize()?;
        let manifest = NativeBundleManifest::decode(
            &read_bounded(&directory.join(MANIFEST_FILE), MAX_MANIFEST_BYTES)?,
            BUILD_CONTRACT,
        )
        .map_err(AotError)?;
        let bytecode = read_bounded(&directory.join(BYTECODE_FILE), MAX_ARTIFACT_BYTES)?;
        let image = read_bounded(&directory.join(IMAGE_FILE), MAX_IMAGE_BYTES)?;
        if hash(&bytecode).as_bytes() != &manifest.bytecode_digest
            || hash(&image).as_bytes() != &manifest.image_digest
        {
            return Err(AotError("bundle content digest mismatch".into()));
        }
        let artifact = KbcArtifact::from_bytes(&bytecode).map_err(|e| AotError(e.to_string()))?;
        if artifact.records().portable_mir.is_some() {
            return Err(AotError(
                "deployment bytecode must not contain compiler input".into(),
            ));
        }
        let program = PreparedProgram::from_artifact(artifact, &Default::default(), cancel)
            .map_err(|e| AotError(e.to_string()))?;
        let target = match manifest.export.as_deref() {
            Some(path) => program.bytecode().exported_function(path),
            None => program.bytecode().entry_function(),
        }
        .ok_or_else(|| AotError("native entry is absent from bytecode exports".into()))?;
        if target.module.index() != manifest.module as usize
            || target.function.index() != manifest.function as usize
        {
            return Err(AotError("native entry differs from bytecode export".into()));
        }
        let function =
            &program.bytecode().modules()[target.module.index()].functions[target.function.index()];
        if function.parameter_count != 0
            || !matches!(
                function.metadata.return_type,
                ValueType::Unit | ValueType::Bool | ValueType::I32
            )
            || function.metadata.effects.may_suspend
            || function.instructions.len() != manifest.safepoints.len()
        {
            return Err(AotError(
                "native signature/points differ from bytecode".into(),
            ));
        }
        check_cancel(cancel)?;
        // SAFETY: the caller supplies trusted immutable deployment inputs. Only
        // explicit DLL-directory and system-directory dependency lookup is enabled.
        let library = unsafe { load_library(&directory.join(IMAGE_FILE))? };
        let address = *unsafe { library.get::<JitCompiledFunction>(manifest.symbol.as_bytes()) }
            .map_err(|e| AotError(e.to_string()))? as usize;
        check_cancel(cancel)?;
        let product = Arc::new(NativeCompilationProduct {
            artifact: manifest.artifact(address),
            owner: Arc::new(LibraryOwner { _library: library }),
        });
        Ok(Self {
            manifest,
            program,
            product,
        })
    }

    pub fn program(&self) -> &PreparedProgram {
        &self.program
    }

    pub fn entry(&self) -> ProgramEntry<'_> {
        self.manifest
            .export
            .as_deref()
            .map_or(ProgramEntry::Main, ProgramEntry::Export)
    }

    /// Install only into a runtime loaded from this exact prepared Program.
    /// Installation is separate from execution and does not run script code.
    pub fn install(
        &self,
        runtime: &KagariRuntime,
        loaded: &LoadedModule,
    ) -> Result<PreparedNativeEntry, AotError> {
        if !self
            .program
            .bytecode()
            .same_version(loaded.verified_program())
        {
            return Err(AotError(
                "bundle belongs to another prepared Program".into(),
            ));
        }
        let (owner, function) = self
            .entry()
            .resolve(loaded)
            .ok_or_else(|| AotError("entry missing".into()))?;
        if owner.slot().index() != self.manifest.module as usize
            || function.index() != self.manifest.function as usize
        {
            return Err(AotError("installed entry differs from bundle".into()));
        }
        // SAFETY: load's caller guarantees code correctness; verified correspondence
        // and the retained library owner bind this exact immutable Program and entry.
        let installed = unsafe {
            runtime
                .runtime()
                .install_native_function(&owner, self.product.clone())
        }
        .map_err(|e| AotError(e.to_string()))?;
        Ok(PreparedNativeEntry::Native(installed))
    }
}

pub(crate) fn check_cancel(cancel: &CancellationToken) -> Result<(), AotError> {
    cancel.check().map_err(|_| AotError("cancelled".into()))
}

pub(crate) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, AotError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err(AotError("bundle file size limit exceeded".into()));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(AotError("bundle file size limit exceeded".into()));
    }
    Ok(bytes)
}

#[cfg(windows)]
unsafe fn load_library(path: &Path) -> Result<Library, AotError> {
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library as WindowsLibrary,
    };
    // SAFETY: propagated trusted immutable deployment obligation from load.
    unsafe {
        WindowsLibrary::load_with_flags(
            path,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
    .map(Into::into)
    .map_err(|e| AotError(e.to_string()))
}

#[cfg(not(windows))]
unsafe fn load_library(_: &Path) -> Result<Library, AotError> {
    Err(AotError("unsupported AOT platform".into()))
}

#[cfg(all(
    test,
    feature = "source",
    feature = "aot-build",
    target_arch = "x86_64",
    target_os = "windows",
    target_env = "msvc"
))]
mod tests;
