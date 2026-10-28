//! Exact payload-plus-trailer allocation retained through detached payload Drop.
use crate::{error::RuntimeError, native::binding::NativeResult};
use std::{
    alloc::{Layout, alloc, dealloc},
    ptr::NonNull,
    sync::Arc,
};

#[derive(Debug)]
pub(in crate::gc) struct Backing {
    pointer: NonNull<u8>,
    layout: Layout,
    payload_bytes: usize,
}

// SAFETY: only Send payloads are placed. Sharing this control record grants no
// payload access: dereferences require the !Sync storage owner or exclusive token.
unsafe impl Send for Backing {}

// SAFETY: shared methods expose raw pointers, never payload/trailer references.
// The non-Clone typed page owner exclusively controls trailer access.
unsafe impl Sync for Backing {}

impl Backing {
    /// Payload starts at zero; the returned offset names its disjoint aligned trailer.
    /// Preserve payload-only runtime charges separately from the full allocator layout.
    pub(super) fn new(payload: Layout, trailer: Layout) -> NativeResult<(Arc<Self>, usize)> {
        let payload = Layout::from_size_align(payload.size().max(1), payload.align())
            .map_err(|_| RuntimeError::resource_limit("payload allocation layout"))?;
        let (layout, offset) = payload
            .extend(trailer)
            .map_err(|_| RuntimeError::resource_limit("payload trailer layout"))?;
        // SAFETY: nonzero checked composite layout; null is rejected before publication.
        let pointer = NonNull::new(unsafe { alloc(layout) })
            .ok_or_else(|| RuntimeError::resource_limit("payload backing"))?;
        Ok((
            Arc::new(Self {
                pointer,
                layout,
                payload_bytes: payload.size(),
            }),
            offset,
        ))
    }

    pub(super) fn pointer(&self) -> NonNull<u8> {
        self.pointer
    }

    pub(super) fn layout(&self) -> Layout {
        self.layout
    }

    pub(super) fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    #[cfg(any(test, feature = "execution-diagnostics"))]
    pub(super) fn control_bytes() -> usize {
        // Arc's two usize counters precede this usize-aligned control record.
        size_of::<Self>() + 2 * size_of::<usize>()
    }
}

impl Drop for Backing {
    fn drop(&mut self) {
        // SAFETY: last control owner; initialized payloads have been dropped/taken.
        unsafe { dealloc(self.pointer.as_ptr(), self.layout) };
    }
}
