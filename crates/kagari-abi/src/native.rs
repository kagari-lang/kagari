use crate::version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackendId(String);

impl BackendId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BackendId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackendTarget {
    pub triple: String,
    pub pointer_width: u8,
    pub features: Vec<String>,
}

impl BackendTarget {
    pub fn new(triple: impl Into<String>, pointer_width: u8) -> Self {
        Self {
            triple: triple.into(),
            pointer_width,
            features: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutableEntryPoint {
    Unresolved,
    Symbol(String),
    Native { symbol: String, address: usize },
}

/// Physical emitted code metadata; semantic function identities live in contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeArtifact {
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub backend: BackendId,
    pub target: BackendTarget,
    pub entry: ExecutableEntryPoint,
    pub traps: Vec<ExecutableTrap>,
}

impl NativeArtifact {
    pub fn new(backend: BackendId, target: BackendTarget) -> Self {
        Self {
            runtime_abi_version: KAGARI_RUNTIME_ABI_VERSION.into(),
            runtime_helper_abi_version: KAGARI_RUNTIME_HELPER_ABI_VERSION.into(),
            backend,
            target,
            entry: ExecutableEntryPoint::Unresolved,
            traps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableTrap {
    pub instruction_offset: usize,
    pub reason: String,
}

/// Keeps executable pages alive for every installed immutable execution version.
/// Backend implementations release their pages only when the final owner is dropped.
pub trait NativeCodeOwner: fmt::Debug {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeType {
    Pointer,
    I32,
    I64,
}

/// Runtime-owned process address. Compiler orchestration supplies the ABI signature.
#[derive(Debug, Clone)]
pub struct NativeHelperSymbol {
    pub symbol: String,
    pub address: usize,
}

#[derive(Debug, Clone)]
pub struct NativeHelperDeclaration {
    pub symbol: String,
    pub address: usize,
    pub parameters: Vec<NativeType>,
    pub results: Vec<NativeType>,
}

/// Explicit symbol bindings are supplied before compilation or script effects.
#[derive(Debug, Clone, Default)]
pub struct NativeLinkDescription {
    pub helpers: Vec<NativeHelperDeclaration>,
}
