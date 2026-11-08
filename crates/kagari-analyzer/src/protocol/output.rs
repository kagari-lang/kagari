//! Shared accounting for retained protocol output and reliable lifecycle headroom.

/// Conservative envelope/metadata allowance, also charged for bounded control frames.
/// Variable semantic payload storage is charged in addition to this base.
pub(crate) const MESSAGE_BASE_CHARGE_BYTES: usize = 4 * 1024;

/// Maximum Unicode scalar values retained in a terminal error message, not UTF-8 bytes.
pub(crate) const MAX_ERROR_MESSAGE_CHARS: usize = 512;

/// Reliable progress begin/end plus one terminal response per admitted request.
pub(crate) const REQUEST_LIFECYCLE_FRAMES: usize = 3;

/// Additional session-wide control headroom beyond ordinary request lifecycles.
pub(crate) const SESSION_CONTROL_RESERVE_FRAMES: usize = 8;

/// Headroom a large result or optional notification must leave for unresolved requests.
pub(crate) const fn request_lifecycle_reserve_bytes(unresolved_requests: usize) -> usize {
    unresolved_requests * REQUEST_LIFECYCLE_FRAMES * MESSAGE_BASE_CHARGE_BYTES
}
