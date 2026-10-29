//! Opaque, non-owning references used only under retained runtime transport.
use crate::gc::object_header::ObjectHeader;
use std::ptr::NonNull;

/// A stable managed allocation address, without independent retention authority.
///
/// Copying this transport neither roots the object nor permits dereferencing it.
/// Runtime entrypoints require a live frame, root or admitted provider lifetime.
/// Addresses never appear in portable artifacts. Taken Native aliases retain their
/// original allocation until ordinary tracing reclaims the tombstone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct GcRef {
    pointer: NonNull<ObjectHeader>,
}

// SAFETY: this opaque carrier offers no safe dereference. Transport between
// threads does not transfer runtime access; admitted providers retain that duty.
unsafe impl Send for GcRef {}

// SAFETY: sharing only copies an address. Actual storage access remains under the
// single runtime owner's borrow and liveness/publication protocol.
unsafe impl Sync for GcRef {}

impl GcRef {
    /// # Safety
    /// The pointer must name a fully initialized live managed header allocated by
    /// the runtime. Keep its original allocation provenance and root/traced owner
    /// until publication; never reconstruct a reference from an integer address.
    pub(crate) unsafe fn from_allocation(pointer: NonNull<u8>) -> Self {
        Self {
            pointer: pointer.cast(),
        }
    }

    pub(crate) fn pointer(self) -> NonNull<u8> {
        self.pointer.cast()
    }
}
