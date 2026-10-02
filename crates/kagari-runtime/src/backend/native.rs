//! Installation and invocation of trusted compiler products without a compiler dependency.
use std::{mem, rc::Rc, sync::Arc};

use kagari_abi::{
    native::{ExecutableEntryPoint, ExecutableFunctionArtifact, NativeCompilationProduct},
    native_call::{
        JIT_STATUS_CANCELLED, JIT_STATUS_ENGINE_FAULT, JIT_STATUS_INTEGER_OVERFLOW,
        JIT_STATUS_INVALID_HEAP_REFERENCE, JIT_STATUS_INVALID_RUNTIME, JIT_STATUS_OK,
        JIT_STATUS_RESOURCE_LIMIT, JitCompiledFunction, JitValue,
    },
    representation::ValueType,
    version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION},
};

use crate::{
    Runtime,
    backend::BackendInvocationError,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    jit_abi::decode_native_value,
    module::{LoadedModule, ModuleEpochRetention, ModuleKey, ModuleStore},
    value::Value,
};

/// Callable native code bound to one runtime and its exact dependency versions.
/// Cloning retains code memory and module instances; its descriptor is immutable.
#[derive(Debug, Clone)]
pub struct InstalledNativeFunction {
    product: Rc<NativeCompilationProduct>,
    module: LoadedModule,
    _retention: Rc<NativeRetention>,
}
impl InstalledNativeFunction {
    pub fn artifact(&self) -> &ExecutableFunctionArtifact {
        &self.product.artifact
    }
    pub fn module(&self) -> &LoadedModule {
        &self.module
    }
}

/// Captured before the native frame unwinds, preserving its failure origin.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{error}")]
pub struct NativeInvocationFailure {
    #[source]
    pub error: BackendInvocationError,
    pub trace: Arc<ErrorTrace>,
}

#[derive(Debug)]
struct NativeRetention {
    store: ModuleStore,
    members: Vec<ModuleKey>,
}
impl Drop for NativeRetention {
    fn drop(&mut self) {
        for &member in &self.members {
            self.store
                .release_epoch(member, ModuleEpochRetention::CompiledArtifact);
        }
    }
}

impl Runtime {
    /// Bind executable memory to this runtime's verified module and dependency closure.
    /// Installation does not invoke code. Unsupported signatures fail before entry.
    ///
    /// # Safety
    /// The product must be generated for this exact verified module and pinned program,
    /// this host target, and the current Kagari native/helper ABI. Its entry must obey
    /// `JitCompiledFunction`, including logical charges, roots and helper contracts.
    /// The owner must retain all referenced executable pages and links while any clone
    /// of the product exists. Native code must not unwind through the C ABI.
    /// Descriptor validation cannot establish these properties for arbitrary pointers.
    pub unsafe fn install_native_function(
        &self,
        module: &LoadedModule,
        product: Rc<NativeCompilationProduct>,
    ) -> Result<InstalledNativeFunction, BackendInvocationError> {
        self.validate_loaded_module(module)
            .map_err(runtime_failure)?;
        self.resources()
            .ensure_execution_allowed()
            .map_err(runtime_failure)?;
        validate_product(module, &product.artifact)?;
        let mut retention = NativeRetention {
            store: self.modules.clone(),
            members: Vec::new(),
        };
        for member in module.members() {
            if !self
                .modules
                .retain_epoch(member.key(), ModuleEpochRetention::CompiledArtifact)
            {
                return Err(runtime_failure(RuntimeError::module_validation(
                    "native dependency was released",
                )));
            }
            retention.members.push(member.key());
        }
        Ok(InstalledNativeFunction {
            product,
            module: module.clone(),
            _retention: Rc::new(retention),
        })
    }

    /// Invoke installed code using the ordinary session and frame stack. Failures
    /// after native entry never request an interpreter restart.
    pub fn invoke_native_function(
        &self,
        installed: &InstalledNativeFunction,
    ) -> Result<Value, NativeInvocationFailure> {
        let failure = |error| NativeInvocationFailure {
            error,
            trace: self.capture_error_trace(),
        };
        self.validate_loaded_module(&installed.module)
            .map_err(runtime_failure)
            .map_err(failure)?;
        self.resources()
            .ensure_execution_allowed()
            .map_err(runtime_failure)
            .map_err(failure)?;
        // The existing native subset has no observer callbacks. Metadata alone
        // does not grant permission to bypass an attached execution observer.
        if self
            .resources
            .active_session()
            .is_some_and(|session| session.observer.borrow().is_some())
        {
            return Err(failure(BackendInvocationError::UnsupportedArtifact(
                "native invocation with an execution observer is unsupported".into(),
            )));
        }
        let stack = self
            .enter_execution_stack(&installed.module)
            .map_err(runtime_failure)
            .map_err(failure)?;
        stack
            .push(
                installed.module.slot(),
                installed.artifact().function,
                &[],
                None,
            )
            .map_err(runtime_failure)
            .map_err(failure)?;
        // Map failures while the frame still exists. The stack guard then unwinds
        // only this call's frames on success, trap, cancellation or budget failure.
        self.call_native_entry(installed).map_err(failure)
    }

    fn call_native_entry(
        &self,
        installed: &InstalledNativeFunction,
    ) -> Result<Value, BackendInvocationError> {
        let ExecutableEntryPoint::Native { address, .. } = installed.artifact().entry else {
            unreachable!("installation validates the entry")
        };
        // SAFETY: installation establishes code lifetime and ABI; the sealed handle
        // retains the product and versions, and the active frame supplies context.
        let function = unsafe { mem::transmute::<usize, JitCompiledFunction>(address) };
        let mut result = JitValue::default();
        let status = unsafe { function((self as *const Self).cast(), &mut result) };
        self.resources
            .ensure_execution_allowed()
            .map_err(runtime_failure)?;
        check_status(self, status)?;
        let value = decode_native_value(result).ok_or_else(|| {
            runtime_failure(
                self.resources
                    .quarantine("compiled function returned an invalid value"),
            )
        })?;
        let expected = installed.module.bytecode.functions[installed.artifact().function.index()]
            .metadata
            .return_type;
        if !value.has_representation(expected) {
            return Err(runtime_failure(
                self.resources
                    .quarantine("compiled function returned the wrong representation"),
            ));
        }
        Ok(value)
    }
}

fn runtime_failure(error: RuntimeError) -> BackendInvocationError {
    BackendInvocationError::RuntimeFailure(error)
}

fn validate_product(
    module: &LoadedModule,
    artifact: &ExecutableFunctionArtifact,
) -> Result<(), BackendInvocationError> {
    let unsupported = |reason: &str| BackendInvocationError::UnsupportedArtifact(reason.into());
    if artifact.runtime_abi_version != KAGARI_RUNTIME_ABI_VERSION
        || artifact.runtime_helper_abi_version != KAGARI_RUNTIME_HELPER_ABI_VERSION
    {
        return Err(unsupported(
            "native product ABI version differs from this runtime",
        ));
    }
    let function = module
        .bytecode
        .functions
        .get(artifact.function.index())
        .filter(|function| function.id == artifact.function)
        .ok_or_else(|| unsupported("native function is absent from the verified module"))?;
    if !matches!(artifact.entry, ExecutableEntryPoint::Native { address, .. } if address != 0) {
        return Err(unsupported("native product has no resolved entry"));
    }
    if usize::from(artifact.target.pointer_width) != usize::BITS as usize {
        return Err(unsupported(
            "native target pointer width differs from this host",
        ));
    }
    if function.parameter_count != 0
        || !matches!(
            function.metadata.return_type,
            ValueType::Unit | ValueType::Bool | ValueType::I32
        )
    {
        return Err(unsupported(
            "native entry requires a zero-argument scalar signature",
        ));
    }
    if artifact
        .safepoints
        .iter()
        .any(|point| point.instruction_offset >= function.instructions.len())
        || artifact
            .traps
            .iter()
            .any(|trap| trap.instruction_offset >= function.instructions.len())
    {
        return Err(unsupported(
            "native point offset is outside the verified function",
        ));
    }
    Ok(())
}

fn check_status(runtime: &Runtime, status: i32) -> Result<(), BackendInvocationError> {
    let error = match status {
        JIT_STATUS_OK => return Ok(()),
        JIT_STATUS_RESOURCE_LIMIT => runtime
            .resources
            .termination()
            .unwrap_or_else(|| RuntimeError::resource_limit("instruction steps")),
        JIT_STATUS_CANCELLED => {
            RuntimeError::new(RuntimeErrorKind::Cancelled, "execution cancelled")
        }
        JIT_STATUS_INTEGER_OVERFLOW => {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "integer overflow")
        }
        JIT_STATUS_INVALID_HEAP_REFERENCE => RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "invalid heap reference at safepoint",
        ),
        JIT_STATUS_ENGINE_FAULT | JIT_STATUS_INVALID_RUNTIME => runtime
            .resources
            .quarantine("native execution reported an engine fault"),
        _ => runtime
            .resources
            .quarantine("compiled function returned an unknown status"),
    };
    Err(runtime_failure(error))
}
