//! Extraction requires an explicit proof that Rust data contains no script edges.
use crate::native::{payload::data::NativeData, storage::NativePayload};

/// Payloads whose original Rust value may leave runtime storage intact.
/// This capability does not require Clone, Copy or Sync.
///
/// # Safety
/// Every reachable field must be free of script values, runtime handles/roots,
/// executable metadata and stored script callbacks, including through interior
/// mutable state, pointers and shared Rust owners. Extraction and subsequent Rust
/// mutation/destruction must not retain or manufacture such edges. Accounting must
/// remain stable while the payload is installed except through checked runtime
/// mutation. An empty `NativePayload::trace` alone does not prove this contract.
/// Ordinary Rust-owned state such as strings or a host-only drop counter is allowed.
pub unsafe trait NativeTake: NativePayload {}

// SAFETY: NativeData already proves a recursively fixed, edge-free representation.
unsafe impl<T: NativeData> NativeTake for T {}
