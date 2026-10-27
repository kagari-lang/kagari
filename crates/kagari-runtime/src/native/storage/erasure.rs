//! Sealed Rust-type operations for placement into nonmoving GC allocations.
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, storage::NativeObject},
};
use std::{alloc::Layout, any::Any, ptr::NonNull};

pub(super) struct Erasure {
    pub(super) layout: Layout,
    pub(super) place: unsafe fn(Box<dyn Any + Send>, NonNull<u8>),
    pub(super) view: unsafe fn(NonNull<u8>) -> NonNull<dyn Any + Send>,
    pub(super) destroy: unsafe fn(NonNull<u8>),
}

impl Erasure {
    pub(super) fn new<T: Any + Send>() -> Self {
        Self {
            layout: Layout::new::<T>(),
            place: |value, destination| {
                let value = value.downcast::<T>().expect("sealed payload TypeId");
                // SAFETY: placement reserves Layout::new::<T>() and transfers ownership once.
                unsafe { destination.cast::<T>().as_ptr().write(*value) };
            },
            view: |pointer| {
                // The fat pointer carries T's vtable, not a borrowed directory reference.
                let pointer = pointer.cast::<T>().as_ptr() as *mut (dyn Any + Send);
                NonNull::new(pointer).expect("non-null payload allocation")
            },
            destroy: |pointer| {
                // SAFETY: the owner clears initialized state before exactly one dispatch.
                unsafe { pointer.cast::<T>().as_ptr().drop_in_place() };
            },
        }
    }
}

pub(super) enum Payload {
    // Admission/factory transport only: never retained as a heap representation.
    Staged(Box<dyn Any + Send>),
    Inline { pointer: NonNull<u8>, loaned: bool },
    Taken,
}

// SAFETY: only Send payloads are admitted. Inline storage belongs to the moved
// Runtime, and its scoped loans cannot cross thread transfer. No Sync is supplied.
unsafe impl Send for Payload {}

impl Payload {
    pub(super) fn view(&self, erasure: &Erasure) -> NativeResult<&(dyn Any + Send)> {
        match self {
            Self::Staged(value) => Ok(value.as_ref()),
            Self::Inline {
                pointer,
                loaned: false,
            } => {
                // SAFETY: initialized inline storage is alive for the header's borrow.
                Ok(unsafe { (erasure.view)(*pointer).as_ref() })
            }
            _ => Err(invalid()),
        }
    }

    pub(super) fn view_mut(&mut self, erasure: &Erasure) -> NativeResult<&mut (dyn Any + Send)> {
        match self {
            Self::Staged(value) => Ok(value.as_mut()),
            Self::Inline {
                pointer,
                loaned: false,
            } => {
                // SAFETY: exclusive header access excludes other payload access/loans.
                Ok(unsafe { (erasure.view)(*pointer).as_mut() })
            }
            _ => Err(invalid()),
        }
    }

    pub(super) fn loan(&mut self, erasure: &Erasure) -> NativeResult<NonNull<dyn Any + Send>> {
        let Self::Inline { pointer, loaned } = self else {
            return Err(invalid());
        };
        if *loaned {
            return Err(invalid());
        }
        *loaned = true;
        // SAFETY: caller owns a guarded non-reentrant loan until end_loan; header
        // access now rejects payload reads. Neither allocation nor GC can run.
        Ok(unsafe { (erasure.view)(*pointer) })
    }

    pub(super) fn end_loan(&mut self) {
        let Self::Inline { loaned, .. } = self else {
            unreachable!("installed native loan")
        };
        assert!(*loaned, "exclusive native loan");
        *loaned = false;
    }
}

impl Drop for NativeObject {
    fn drop(&mut self) {
        if let Payload::Inline { pointer, loaned } = self.payload {
            assert!(!loaned, "native destruction during a payload loan");
            self.payload = Payload::Taken;
            // SAFETY: the allocation token holds backing through header destruction;
            // initialized ownership is cleared before invoking possibly panicking Drop.
            unsafe { (self.storage.entries.erasure.destroy)(pointer) };
        }
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native payload is loaned or no longer initialized")
}
