//! Immutable UTF-8 text owned by the managed graph.
use crate::value::{
    carriers::{ManagedRepresentation, ScopedValue},
    kind::ObjectKind,
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::array;

#[derive(Debug)]
pub(crate) struct StringObject {
    text: String,
}

impl StringObject {
    pub(crate) fn alloc<'gc>(cx: &mut Scope<'gc>, text: String) -> Result<Gc<'gc, Self>, GcError> {
        cx.alloc_leaf(Self { text })
    }

    /// Lend UTF-8 only inside the protected provider's closed payload loan.
    pub(crate) fn read<'gc, R>(
        cx: &Scope<'gc>,
        value: ScopedValue<'gc>,
        read: impl for<'text> FnOnce(&'text str) -> R,
    ) -> Result<R, GcError> {
        let object = cx.downcast::<Self>(value.object().ok_or(GcError::Representation)?)?;
        Ok(cx.read(object, |object| read(object.as_str())))
    }

    /// Lend several protected strings together, including repeated aliases.
    /// Every payload loan ends before the caller regains mutable services.
    pub(crate) fn read_many<'gc, R, const N: usize>(
        cx: &Scope<'gc>,
        values: &[ScopedValue<'gc>; N],
        read: impl FnOnce([&str; N]) -> R,
    ) -> Result<R, GcError> {
        read_next(cx, values, 0, [""; N], read)
    }

    /// Borrow only through the owning Scope's payload callback.
    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }
}

fn read_next<'gc, R, const N: usize>(
    cx: &Scope<'gc>,
    values: &[ScopedValue<'gc>; N],
    index: usize,
    previous: [&str; N],
    read: impl FnOnce([&str; N]) -> R,
) -> Result<R, GcError> {
    if index == N {
        return Ok(read(previous));
    }
    StringObject::read(cx, values[index], |text| {
        let mut next = array::from_fn(|index| previous[index]);
        next[index] = text;
        read_next(cx, values, index + 1, next, read)
    })?
}

impl ManagedRepresentation for StringObject {
    const KIND: ObjectKind = ObjectKind::String;
}

// SAFETY: text is immutable edge-free UTF-8 backing. Charging is constant-time,
// and Drop releases only the owned String allocation without accessing peers.
unsafe impl GcObject for StringObject {
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
            bytes: self.text.capacity(),
            units: self.text.len(),
        }
    }
}
