//! Opaque, non-owning references used only under retained runtime transport.
use crate::gc::{HeapObjectId, object_header::ObjectHeader};
use std::ptr::NonNull;

/// A stable struct allocation address, without independent retention authority.
///
/// Copying this transport neither roots the object nor permits dereferencing it.
/// Runtime entrypoints require a live frame, root or admitted provider lifetime.
/// Addresses never appear in portable artifacts. Other managed families still
/// use checked IDs until the complete graph migration.
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
    /// The pointer must name a fully initialized live struct header allocated by
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

/// Non-owning identity used while inspecting a protected mixed heap graph.
/// Struct addresses require a live traced owner through any runtime query; they
/// are not generation-checked capabilities after reclamation. Indexed families
/// retain their existing generation validation until their transport migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeapObjectIdentity {
    Indexed(HeapObjectId),
    Struct(GcRef),
}

impl From<HeapObjectId> for HeapObjectIdentity {
    fn from(id: HeapObjectId) -> Self {
        Self::Indexed(id)
    }
}

impl From<GcRef> for HeapObjectIdentity {
    fn from(reference: GcRef) -> Self {
        Self::Struct(reference)
    }
}
