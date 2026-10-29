//! Allocation metadata precedes the initialized packed struct payload.
use std::{alloc::Layout, ptr::NonNull};

/// IDs belong to the existing descriptor and physical allocation directories.
/// They are retained until detached disposal finishes, not just until unmarking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(super) struct ObjectHeader {
    pub(super) descriptor_id: u32,
    pub(super) allocation_id: u32,
}

impl ObjectHeader {
    pub(super) const SIZE: usize = size_of::<Self>();

    pub(super) fn struct_layout(payload_size: usize) -> Option<Layout> {
        Layout::from_size_align(Self::SIZE.checked_add(payload_size)?, align_of::<Self>()).ok()
    }

    /// Initialize the header after the payload is complete, before publication.
    ///
    /// # Safety
    /// `pointer` names uniquely writable, aligned header storage in a reserved
    /// allocation. Both IDs must remain admitted through final disposal.
    pub(super) unsafe fn write(self, pointer: NonNull<u8>) {
        // SAFETY: the allocation owner supplies exclusive initialized placement.
        unsafe { pointer.cast::<Self>().as_ptr().write(self) };
    }

    /// Read a live allocation header without constructing an escaping reference.
    ///
    /// # Safety
    /// The owner must retain the initialized aligned allocation and exclude writes.
    pub(super) unsafe fn read(pointer: NonNull<u8>) -> Self {
        // SAFETY: the caller retains the published allocation under its storage loan.
        unsafe { pointer.cast::<Self>().as_ptr().read() }
    }
}

const _: () = assert!(ObjectHeader::SIZE == 8);
