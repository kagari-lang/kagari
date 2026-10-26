//! Exact checked raw allocation shared only to retain bytes through detached Drop.
use crate::{error::RuntimeError, native::binding::NativeResult};
use std::{
    alloc::{Layout, alloc, dealloc},
    ptr::NonNull,
    sync::Arc,
};

#[derive(Debug)]
pub(in crate::gc) struct Backing {
    pub(super) pointer: NonNull<u8>,
    pub(super) layout: Layout,
}

// SAFETY: only Send payloads are placed. Sharing this control record grants no
// payload access: dereferences require the !Sync storage owner or exclusive token.
unsafe impl Send for Backing {}

// SAFETY: shared methods expose raw pointers, never references to payload bytes.
unsafe impl Sync for Backing {}

impl Backing {
    pub(super) fn new(layout: Layout) -> NativeResult<Arc<Self>> {
        let layout = Layout::from_size_align(layout.size().max(1), layout.align())
            .map_err(|_| RuntimeError::resource_limit("payload allocation layout"))?;
        // SAFETY: nonzero valid layout, null is rejected before publication.
        let pointer = NonNull::new(unsafe { alloc(layout) })
            .ok_or_else(|| RuntimeError::resource_limit("payload backing"))?;
        Ok(Arc::new(Self { pointer, layout }))
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
