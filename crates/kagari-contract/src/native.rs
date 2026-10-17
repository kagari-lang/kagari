//! Native products paired with checked logical function and debug metadata.
use crate::ids::{DebugPointId, FunctionRef};
use kagari_abi::native::{BackendId, BackendTarget, NativeArtifact, NativeCodeOwner};
use std::sync::Arc;

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
pub struct ExecutableFunctionArtifact {
    pub code: NativeArtifact,
    pub function: FunctionRef,
    pub safepoints: Vec<ExecutableSafepoint>,
    pub debug: ExecutableDebugInfo,
}

impl ExecutableFunctionArtifact {
    pub fn new(backend: BackendId, target: BackendTarget, function: FunctionRef) -> Self {
        Self {
            code: NativeArtifact::new(backend, target),
            function,
            safepoints: Vec::new(),
            debug: ExecutableDebugInfo::default(),
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

/// Compiler output pairs executable metadata with its memory lifetime.
/// Finalized code and its owner may be shared across threads and installations.
/// Each invocation still requires the owning runtime's checked execution context.
#[derive(Debug, Clone)]
pub struct NativeCompilationProduct {
    pub artifact: ExecutableFunctionArtifact,
    pub owner: Arc<dyn NativeCodeOwner>,
}
