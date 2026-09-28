use crate::{
    ids::{DebugPointId, FunctionRef},
    version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION},
};
use std::{fmt, rc::Rc};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableSafepoint {
    pub instruction_offset: usize,
    pub kind: ExecutableSafepointKind,
    pub stack_map: ExecutableStackMap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutableSafepointKind {
    RuntimeHelperCall { helper: String },
    CallBoundary,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutableStackMap {
    pub live_slots: Vec<ExecutableStackMapSlot>,
}

impl ExecutableStackMap {
    pub fn empty() -> Self {
        Self {
            live_slots: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableStackMapSlot {
    pub location: ExecutableStackMapLocation,
    pub value_kind: ExecutableStackValueKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutableStackMapLocation {
    Register(u32),
    Local(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutableStackValueKind {
    GcManaged,
    Interface,
    HostHandle,
    HostPathView,
    Ephemeral,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableTrap {
    pub instruction_offset: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableFunctionArtifact {
    pub runtime_abi_version: String,
    pub runtime_helper_abi_version: String,
    pub backend: BackendId,
    pub target: BackendTarget,
    pub function: FunctionRef,
    pub entry: ExecutableEntryPoint,
    pub safepoints: Vec<ExecutableSafepoint>,
    pub debug: ExecutableDebugInfo,
    pub traps: Vec<ExecutableTrap>,
}

impl ExecutableFunctionArtifact {
    pub fn new(backend: BackendId, target: BackendTarget, function: FunctionRef) -> Self {
        Self {
            runtime_abi_version: KAGARI_RUNTIME_ABI_VERSION.into(),
            runtime_helper_abi_version: KAGARI_RUNTIME_HELPER_ABI_VERSION.into(),
            backend,
            target,
            function,
            entry: ExecutableEntryPoint::Unresolved,
            safepoints: Vec::new(),
            debug: ExecutableDebugInfo::default(),
            traps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutableDebugInfo {
    pub has_line_tables: bool,
    pub has_source_spans: bool,
    pub has_live_value_locations: bool,
    pub has_safe_debug_callbacks: bool,
    pub safe_debug_points: Vec<ExecutableDebugPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableDebugPoint {
    pub instruction_offset: usize,
    pub debug_point: DebugPointId,
}

/// Keeps executable pages alive for every installed immutable execution version.
/// Backend implementations release their pages only when the final owner is dropped.
pub trait NativeCodeOwner: fmt::Debug {}

/// Compiler output pairs executable metadata with its memory lifetime.
/// Sharing is within the host thread, matching prepared programs and runtime
/// installations; this contract does not authorize cross-thread execution.
#[derive(Debug, Clone)]
pub struct NativeCompilationProduct {
    pub artifact: ExecutableFunctionArtifact,
    pub owner: Rc<dyn NativeCodeOwner>,
}

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
