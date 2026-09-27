//! Compile once per immutable program/configuration; install separately per runtime.
use std::rc::Rc;

use kagari_abi::ids::FunctionRef;
use kagari_abi::native::{ExecutableEntryPoint, NativeCompilationProduct};
use kagari_abi::version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION};
use kagari_bytecode::ModuleRef;
use kagari_codegen::{
    BackendCompileError, BackendConfiguration, BackendFunctionInput, CodegenBackend,
};
use kagari_common::cancellation::CancellationToken;
use kagari_compiler::native_links::{NativeLinkError, build_native_links};
use kagari_mir::ids::InstanceId;
use kagari_runtime::jit_abi::native_helper_symbols;
use kagari_runtime::{BackendInvocationError, LoadedModule, RuntimeError};
use kagari_vm::PreparedNativeEntry;

use crate::program::PreparedProgram;
use crate::runtime::KagariRuntime;

const MAX_CACHED_FUNCTIONS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct NativeCacheKey {
    module: ModuleRef,
    function: FunctionRef,
    configuration: BackendConfiguration,
    runtime_abi: &'static str,
    helper_abi: &'static str,
}

#[derive(Debug, Clone)]
pub(super) enum CachedFunction {
    Native(Rc<NativeCompilationProduct>),
    Unsupported(Vec<String>),
}

#[derive(Debug, thiserror::Error)]
pub enum NativePreparationError {
    #[error("native preparation cancelled")]
    Cancelled,
    #[error("loaded program does not belong to this prepared version")]
    WrongVersion,
    #[error("entry function `{0}` not found")]
    MissingEntry(String),
    #[error("native compilation failed: {0:?}")]
    Compile(BackendCompileError),
    #[error("native compiler product does not match its requested contract")]
    InvalidProduct,
    #[error("native compilation cache entry limit exceeded")]
    CacheLimit,
    #[error("native link construction failed: {0}")]
    Links(#[from] NativeLinkError),
    #[error("native installation failed: {0}")]
    Installation(#[from] BackendInvocationError),
    #[error("runtime validation failed: {0}")]
    Runtime(#[from] RuntimeError),
}

impl KagariRuntime {
    /// Preparation performs no script execution. Trusted backend implementations
    /// supply products; runtime installation binds them to this runtime's versions.
    pub fn prepare_native(
        &self,
        program: &PreparedProgram,
        module: &LoadedModule,
        entry: &str,
        backend: &mut dyn CodegenBackend,
        cancel: &CancellationToken,
    ) -> Result<PreparedNativeEntry, NativePreparationError> {
        check_cancel(cancel)?;
        self.runtime().validate_loaded_module(module)?;
        if !program.bytecode().same_version(module.verified_program()) {
            return Err(NativePreparationError::WrongVersion);
        }
        let function = module
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == entry)
            .ok_or_else(|| NativePreparationError::MissingEntry(entry.into()))?
            .id;
        let configuration = backend.configuration();
        let unsupported = |message: String| PreparedNativeEntry::Unsupported {
            backend: configuration.backend.clone(),
            diagnostics: vec![message],
        };
        if let Err(error) = self.runtime().validate_jit_boundary() {
            return Ok(unsupported(format!(
                "JIT disabled by runtime policy: {error}"
            )));
        }
        if program.state.mir.is_none() {
            return Ok(unsupported("artifact has no portable native input".into()));
        }
        let key = NativeCacheKey {
            module: module.slot(),
            function,
            configuration: configuration.clone(),
            runtime_abi: KAGARI_RUNTIME_ABI_VERSION,
            helper_abi: KAGARI_RUNTIME_HELPER_ABI_VERSION,
        };
        let cached = program.compile_function(key, backend, cancel)?;
        check_cancel(cancel)?;
        match cached {
            CachedFunction::Unsupported(diagnostics) => Ok(PreparedNativeEntry::Unsupported {
                backend: configuration.backend,
                diagnostics,
            }),
            CachedFunction::Native(product) => {
                // SAFETY: the unsafe backend contract establishes callable code and
                // retained memory. Canonical correspondence and shared version identity
                // bind the exact function/program; symbols come from runtime itself.
                let installed = unsafe { self.runtime().install_native_function(module, product)? };
                Ok(PreparedNativeEntry::Native(installed))
            }
        }
    }
}

impl PreparedProgram {
    fn compile_function(
        &self,
        key: NativeCacheKey,
        backend: &mut dyn CodegenBackend,
        cancel: &CancellationToken,
    ) -> Result<CachedFunction, NativePreparationError> {
        if let Some(cached) = self.state.native.borrow().get(&key).cloned() {
            return Ok(cached);
        }
        if self.state.native.borrow().len() >= MAX_CACHED_FUNCTIONS {
            return Err(NativePreparationError::CacheLimit);
        }
        let mir = self
            .state
            .mir
            .as_ref()
            .expect("native input checked before compilation");
        let module = &mir.modules()[key.module.index()];
        let links = build_native_links(&native_helper_symbols())?;
        let input =
            BackendFunctionInput::new(module, InstanceId::new(key.function.index()), &links)
                .ok_or(NativePreparationError::InvalidProduct)?;
        let result = backend.compile_function(input);
        check_cancel(cancel)?;
        if backend.configuration() != key.configuration {
            return Err(NativePreparationError::InvalidProduct);
        }
        let cached = match result {
            Ok(product) => {
                let descriptor = &product.artifact;
                if descriptor.backend != key.configuration.backend
                    || descriptor.target != key.configuration.target
                    || descriptor.function != key.function
                    || descriptor.runtime_abi_version != key.runtime_abi
                    || descriptor.runtime_helper_abi_version != key.helper_abi
                    || !matches!(descriptor.entry, ExecutableEntryPoint::Native { address, .. } if address != 0)
                {
                    return Err(NativePreparationError::InvalidProduct);
                }
                CachedFunction::Native(Rc::new(product))
            }
            Err(error) if error.is_unsupported() => CachedFunction::Unsupported(
                error
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect(),
            ),
            Err(error) => return Err(NativePreparationError::Compile(error)),
        };
        // Backend callbacks run without a cache borrow. Reentrant host preparation
        // may have populated this key or exhausted capacity while compilation ran.
        let mut cache = self.state.native.borrow_mut();
        if let Some(existing) = cache.get(&key) {
            return Ok(existing.clone());
        }
        if cache.len() >= MAX_CACHED_FUNCTIONS {
            return Err(NativePreparationError::CacheLimit);
        }
        cache.insert(key, cached.clone());
        Ok(cached)
    }
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), NativePreparationError> {
    cancel
        .check()
        .map_err(|_| NativePreparationError::Cancelled)
}
