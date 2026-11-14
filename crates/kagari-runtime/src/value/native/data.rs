//! Fixed Rust data has a separate, edge-free physical provider.
use crate::native::payload::data::NativeData;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    thread,
};

#[derive(Debug)]
pub(crate) struct DataObject<T: NativeData>(T);

impl<T: NativeData> DataObject<T> {
    pub(crate) fn alloc<'gc>(cx: &mut Scope<'gc>, value: T) -> Result<Gc<'gc, Self>, GcError> {
        cx.alloc_leaf(Self(value))
    }

    pub(crate) fn value(&self) -> &T {
        &self.0
    }

    /// A user panic does not invalidate fixed edge-free storage. Catch it inside
    /// the provider transaction, commit unchanged charge, then resume only after
    /// the caller releases all heap loans. Completed scalar writes stay visible.
    pub(crate) fn edit<'gc, R>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        edit: impl for<'data> FnOnce(&'data mut T) -> R,
    ) -> Result<thread::Result<R>, GcError> {
        // SAFETY: NativeData excludes edges, indirect allocations and interior
        // mutation. Every possible edit preserves charge and scanner facts.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let result = run_edit(&mut owner.0, edit);
                mutation.continue_scan();
                Ok(result)
            })
        }
    }
}

// SAFETY: NativeData's recursive contract excludes graph edges and backing. Its
// data is inline in this GC allocation; user editing cannot change those facts.
unsafe impl<T: NativeData> GcObject for DataObject<T> {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: 0,
            units: size_of::<T>().div_ceil(size_of::<usize>()),
        }
    }
}

pub(crate) fn run_edit<T: NativeData, R>(
    data: &mut T,
    edit: impl FnOnce(&mut T) -> R,
) -> thread::Result<R> {
    catch_unwind(AssertUnwindSafe(|| edit(data)))
}

#[cfg(test)]
mod tests;
