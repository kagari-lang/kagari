//! Object emission retains symbolic metadata, never executable process addresses.
use crate::{BackendConfiguration, BackendFunctionInput, diagnostic::BackendCompileError};
use kagari_contract::native::ExecutableFunctionArtifact;

#[derive(Debug)]
pub struct NativeObjectProduct {
    pub artifact: ExecutableFunctionArtifact,
    pub bytes: Vec<u8>,
}

/// Trusted native object compiler consuming the same verified facts as JIT.
///
/// # Safety
/// Objects must implement the exact selected function and native ABI when linked
/// for the declared target. Symbolic metadata must describe that code faithfully.
/// Emission must not embed process addresses or execute script code.
pub unsafe trait ObjectBackend {
    fn configuration(&self) -> BackendConfiguration;

    fn emit_object(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<NativeObjectProduct, BackendCompileError>;
}
