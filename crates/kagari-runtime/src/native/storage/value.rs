//! Opaque, nonowning edges in an admitted Native payload.
use crate::{
    native::{binding::NativeResult, scalar::NativeScalar},
    value::Value,
};
use std::slice;

/// A scalar or traced script edge owned by a Native payload's scanner.
///
/// Copying this carrier creates neither a root nor independent access authority.
/// Store it in scanned payload or scratch storage before its source can change.
/// Use `StorageAccess::retain` before taking an element across a callback or out
/// of its owner's lifetime. Scalar conversion never dereferences script storage.
/// Equality compares carriers, not the language's structural equality operation.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(transparent)]
pub struct StoredValue(Value);

impl StoredValue {
    /// Construct an edge-free scalar without allocating or registering a root.
    pub fn scalar<T: NativeScalar>(value: T) -> Self {
        Self(value.encode())
    }

    /// Decode an exact scalar carrier, checking its Rust range where required.
    /// Managed values are rejected without querying their referents.
    pub fn as_scalar<T: NativeScalar>(&self) -> NativeResult<T> {
        T::decode(self.0)
    }

    /// Borrow runtime transport without inspecting any referent. The transparent
    /// carriers have identical validity and layout; the original owner remains
    /// responsible for tracing and publication across use of the borrowed slice.
    pub(crate) fn values(values: &[Self]) -> &[Value] {
        // SAFETY: repr(transparent), immutable borrowing, and the same initialized
        // carrier array. No pointer bits are reconstructed or dereferenced.
        unsafe { slice::from_raw_parts(values.as_ptr().cast(), values.len()) }
    }

    pub(crate) fn new(value: Value) -> Self {
        Self(value)
    }

    pub(crate) fn value(&self) -> Value {
        self.0
    }

    pub(crate) fn value_ref(&self) -> &Value {
        &self.0
    }

    /// Read transport under the unsafe provider's independently protected owner.
    ///
    /// # Safety
    /// Keep the original traced source or a retained root alive through every use
    /// of the returned transport. Source replacement, collection and callbacks
    /// must not remove its last protection. This operation acquires no root and
    /// cannot validate a carrier after its referent has been reclaimed.
    #[doc(hidden)]
    pub unsafe fn value_transport(&self) -> Value {
        self.0
    }
}
