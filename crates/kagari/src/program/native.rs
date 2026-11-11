//! Compile immutable products outside Runtime; bind native authority at installation.
use crate::{program::Compilation, runtime::Runtime};
use kagari_abi::{
    native::{BackendId, ExecutableEntryPoint, NativeLinkDescription},
    version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION},
};
use kagari_bytecode::program::ModuleRef;
use kagari_codegen::{
    BackendConfiguration, BackendFunctionInput, CodegenBackend, diagnostic::BackendCompileError,
};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::{ids::FunctionRef, native::NativeCompilationProduct};
use kagari_mir::ids::InstanceId;
use kagari_runtime::{
    backend::BackendInvocationError,
    error::RuntimeError,
    module::{InstalledProgram, entry::ProgramEntry, revision::ProgramRevision},
};
use kagari_vm::vm::native::PreparedNativeEntry;
use std::{collections::HashMap, sync::Arc};

const MAX_CACHED_FUNCTIONS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NativeCacheKey {
    revision: ProgramRevision,
    module: ModuleRef,
    function: FunctionRef,
    configuration: BackendConfiguration,
    runtime_abi: &'static str,
    helper_abi: &'static str,
}

#[derive(Debug, Clone)]
enum CachedFunction {
    Native(Arc<NativeCompilationProduct>),
    Unsupported(Vec<String>),
}

/// Preparation-owned cache with at most 4096 exact revision/function/configuration
/// decisions. Dropping it releases cached pages; installed entries retain their own
/// page owners. Cache keys retain identity tokens, not source/MIR/bytecode payloads.
/// Execution never accesses this cache. Create a fresh compiler for a new batch
/// when its bounded cache is no longer useful.
#[derive(Debug, Default)]
pub struct NativeCompiler {
    cache: HashMap<NativeCacheKey, CachedFunction>,
}

/// Immutable finalized native decision, safe to distribute to independent owners.
/// It carries no Runtime binding, local policy or execution permission. Installation
/// must check the exact revision before exposing a native entry.
#[derive(Debug, Clone)]
pub struct NativeCode {
    revision: ProgramRevision,
    root: ModuleRef,
    module: ModuleRef,
    function: FunctionRef,
    backend: BackendId,
    compiled: CachedFunction,
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
    #[error("native installation failed: {0}")]
    Installation(#[from] BackendInvocationError),
    #[error("runtime validation failed: {0}")]
    Runtime(#[from] RuntimeError),
}

impl NativeCompiler {
    /// Resolve a verified entry and compile once for its exact product/configuration.
    /// No Runtime, script effect or local execution policy participates. Missing
    /// entries, cancellation, malformed backend products and cache exhaustion reject
    /// without inserting a decision. Unsupported bodies retain a checked fallback.
    pub fn prepare<'a>(
        &mut self,
        compilation: &Compilation,
        entry: impl Into<ProgramEntry<'a>>,
        backend: &mut dyn CodegenBackend,
        cancel: &CancellationToken,
    ) -> Result<NativeCode, NativePreparationError> {
        check_cancel(cancel)?;
        let entry = entry.into();
        let program = compilation.program();
        let target = entry
            .resolve_program(program.bytecode())
            .ok_or_else(|| NativePreparationError::MissingEntry(entry.label().into()))?;
        let configuration = backend.configuration();
        let revision = program.bytecode().revision();
        let backend_id = configuration.backend.clone();
        let function =
            &program.bytecode().modules()[target.module.index()].functions[target.function.index()];
        let compiled = if function.metadata.effects.may_suspend {
            CachedFunction::Unsupported(vec![
                "resume bodies require the owned interpreter driver".into(),
            ])
        } else if compilation.mir.is_none() {
            CachedFunction::Unsupported(vec!["artifact has no portable native input".into()])
        } else {
            let key = NativeCacheKey {
                revision: revision.clone(),
                module: target.module,
                function: target.function,
                configuration,
                runtime_abi: KAGARI_RUNTIME_ABI_VERSION,
                helper_abi: KAGARI_RUNTIME_HELPER_ABI_VERSION,
            };
            self.compile_function(compilation, key, backend, cancel)?
        };
        check_cancel(cancel)?;
        Ok(NativeCode {
            revision,
            root: program.bytecode().root(),
            module: target.module,
            function: target.function,
            backend: backend_id,
            compiled,
        })
    }

    fn compile_function(
        &mut self,
        compilation: &Compilation,
        key: NativeCacheKey,
        backend: &mut dyn CodegenBackend,
        cancel: &CancellationToken,
    ) -> Result<CachedFunction, NativePreparationError> {
        if let Some(cached) = self.cache.get(&key) {
            return Ok(cached.clone());
        }
        if self.cache.len() >= MAX_CACHED_FUNCTIONS {
            return Err(NativePreparationError::CacheLimit);
        }
        let mir = compilation
            .mir
            .as_ref()
            .expect("native input checked before compilation");
        let module = &mir.modules()[key.module.index()];
        let links = NativeLinkDescription::scalar();
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
                if descriptor.code.backend != key.configuration.backend
                    || descriptor.code.target != key.configuration.target
                    || descriptor.function != key.function
                    || descriptor.code.runtime_abi_version != key.runtime_abi
                    || descriptor.code.runtime_helper_abi_version != key.helper_abi
                    || !matches!(descriptor.code.entry, ExecutableEntryPoint::Native { address, .. } if address != 0)
                {
                    return Err(NativePreparationError::InvalidProduct);
                }
                CachedFunction::Native(Arc::new(product))
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
        self.cache.insert(key, cached.clone());
        Ok(cached)
    }
}

impl Runtime {
    /// Bind a finalized decision to this installation without compiling. Exact
    /// revision and local authority are validated before local policy may choose
    /// fallback. Installed entries retain pages independently of the compiler/code.
    pub fn install_native(
        &mut self,
        installed: &InstalledProgram,
        code: &NativeCode,
    ) -> Result<PreparedNativeEntry, NativePreparationError> {
        let module = self.runtime().select_program(installed)?;
        if module.verified_program().revision() != code.revision
            || module.verified_program().root() != code.root
        {
            return Err(NativePreparationError::WrongVersion);
        }
        let owner = module
            .member(code.module)
            .filter(|owner| {
                owner
                    .bytecode
                    .functions
                    .get(code.function.index())
                    .is_some()
            })
            .ok_or(NativePreparationError::InvalidProduct)?;
        let unsupported = |message: String| PreparedNativeEntry::Unsupported {
            backend: code.backend.clone(),
            diagnostics: vec![message],
        };
        if let Some(reason) = self.default_context().jit_policy.fallback_reason() {
            return Ok(unsupported(reason.into()));
        }
        if let Err(error) = self.runtime().resources().ensure_execution_allowed() {
            return Ok(unsupported(format!(
                "JIT disabled by runtime policy: {error}"
            )));
        }
        match &code.compiled {
            CachedFunction::Unsupported(diagnostics) => Ok(PreparedNativeEntry::Unsupported {
                backend: code.backend.clone(),
                diagnostics: diagnostics.clone(),
            }),
            CachedFunction::Native(product) => {
                // SAFETY: the trusted backend establishes callable pages; the opaque
                // revision binds their exact verified body before local installation.
                let installed = unsafe {
                    self.runtime()
                        .install_native_function(&owner, product.clone())?
                };
                Ok(PreparedNativeEntry::Native(installed))
            }
        }
    }
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), NativePreparationError> {
    cancel
        .check()
        .map_err(|_| NativePreparationError::Cancelled)
}
