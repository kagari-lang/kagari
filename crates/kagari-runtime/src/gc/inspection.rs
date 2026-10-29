//! Scoped host inspection keeps internal object transport inside the runtime.
#[cfg(test)]
mod tests;

use crate::{
    Runtime,
    error::RuntimeError,
    gc::{
        GcHeap,
        leases::StorageBorrow,
        roots::{RootSet, RootedValue},
    },
    native::binding::NativeResult,
    value::{Value, ValueCategory},
};
use std::{cell::Ref, fmt, marker::PhantomData};

/// A checked value available only during a non-reentrant inspection loan.
///
/// Scalar reads copy data. String reads borrow storage until the returned view
/// is dropped. Neither managed transport nor a borrowed view can escape the
/// inspection closure. Inspection cannot allocate script objects, collect,
/// invoke script or edit storage, including through a captured runtime alias.
/// Use owning typed conversions for work that needs those operations.
///
/// ```compile_fail
/// use kagari_runtime::{Runtime, gc::roots::RootedValue};
/// fn escape(runtime: &Runtime, root: &RootedValue) {
///     let _view = root.inspect(runtime, |view| Ok(view));
/// }
/// ```
///
/// ```compile_fail
/// use kagari_runtime::{Runtime, gc::roots::RootedValue};
/// fn escape_text(runtime: &Runtime, root: &RootedValue) {
///     let _text = root.inspect(runtime, |view| view.as_str());
/// }
/// ```
pub struct ValueView<'view> {
    heap: &'view GcHeap,
    value: Value,
    invariant: PhantomData<fn(&'view ()) -> &'view ()>,
}

/// Debug reports transport tags and scalar data, not recursive heap contents.
/// Formatting remains inside the inspection loan, including writer callbacks.
impl fmt::Debug for ValueView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(formatter)
    }
}

impl ValueView<'_> {
    /// Classify the value without exposing a heap identity.
    pub fn category(&self) -> ValueCategory {
        self.value.category()
    }

    /// Whether this is the unit value.
    pub fn is_unit(&self) -> bool {
        matches!(self.value, Value::Unit)
    }

    /// Copy an exact Bool value; other representations are rejected.
    pub fn as_bool(&self) -> NativeResult<bool> {
        match self.value {
            Value::Bool(value) => Ok(value),
            _ => Err(type_error("Bool")),
        }
    }

    /// Copy an exact Char value; other representations are rejected.
    pub fn as_char(&self) -> NativeResult<char> {
        match self.value {
            Value::Char(value) => Ok(value),
            _ => Err(type_error("Char")),
        }
    }

    /// Copy an exact I32 value without numeric conversion.
    pub fn as_i32(&self) -> NativeResult<i32> {
        match self.value {
            Value::I32(value) => Ok(value),
            _ => Err(type_error("I32")),
        }
    }

    /// Copy an exact I64 value without numeric conversion.
    pub fn as_i64(&self) -> NativeResult<i64> {
        match self.value {
            Value::I64(value) => Ok(value),
            _ => Err(type_error("I64")),
        }
    }

    /// Copy an exact U64 value without numeric conversion.
    pub fn as_u64(&self) -> NativeResult<u64> {
        match self.value {
            Value::U64(value) => Ok(value),
            _ => Err(type_error("U64")),
        }
    }

    /// Copy an exact F32 value without changing its bits.
    pub fn as_f32(&self) -> NativeResult<f32> {
        match self.value {
            Value::F32(value) => Ok(value),
            _ => Err(type_error("F32")),
        }
    }

    /// Copy an exact F64 value without changing its bits.
    pub fn as_f64(&self) -> NativeResult<f64> {
        match self.value {
            Value::F64(value) => Ok(value),
            _ => Err(type_error("F64")),
        }
    }

    /// Borrow string bytes within this inspection; other kinds are rejected.
    /// Copy with `to_owned` inside the closure when owned Rust text is needed.
    pub fn as_str(&self) -> NativeResult<Ref<'_, str>> {
        let Value::Str(id) = self.value else {
            return Err(type_error("String"));
        };
        // The inspection owns storage exclusion. Do not re-enter the ordinary
        // string accessor, which correctly rejects competing storage loans.
        self.heap
            .borrow_string_storage(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid inspected string"))
    }
}

impl RootedValue {
    /// Inspect a retained value through a short, non-reentrant loan.
    ///
    /// Rejects foreign or expired roots and unavailable storage before
    /// calling `read`. Aliases cannot replace the source root during the loan.
    /// Closed or cancelled execution does not prevent reading retained data.
    /// Errors and Rust unwinds release the loan before propagating. Return owned
    /// Rust data, or keep an existing owning handle, rather than an internal view.
    pub fn inspect<R>(
        &self,
        runtime: &Runtime,
        read: impl for<'view> FnOnce(ValueView<'view>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let heap = runtime.gc();
        let value = self
            .value(heap)
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired inspected root"))?;
        inspect_value(heap, value, read)
    }
}

impl RootSet {
    /// Inspect one execution slot without exporting managed transport.
    ///
    /// Rejects foreign roots, missing slots and unavailable storage. The same
    /// non-reentrant loan as RootedValue inspection prevents alias replacement.
    /// Scoped capabilities may be classified but their original host lifetime
    /// is not extended by this root pack.
    ///
    /// ```compile_fail
    /// use kagari_runtime::{Runtime, gc::roots::RootSet};
    /// fn escape(runtime: &Runtime, roots: &RootSet) {
    ///     let _view = roots.inspect(runtime, 0, |view| Ok(view));
    /// }
    /// ```
    pub fn inspect<R>(
        &self,
        runtime: &Runtime,
        index: usize,
        read: impl for<'view> FnOnce(ValueView<'view>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let heap = runtime.gc();
        let value = self.get(heap, index).ok_or_else(|| {
            RuntimeError::module_validation("foreign, expired or missing inspected root slot")
        })?;
        inspect_value(heap, value, read)
    }
}

/// The caller retains `value` until this callback-free admission starts, and
/// keeps its original protection for the entire loan. Root mutation is excluded
/// by the same storage guard as collection, reentry and Native payload loans.
pub(crate) fn inspect_value<R>(
    heap: &GcHeap,
    value: Value,
    read: impl for<'view> FnOnce(ValueView<'view>) -> NativeResult<R>,
) -> NativeResult<R> {
    heap.ensure_no_storage_borrow()?;
    heap.resources.ensure_inspection_allowed()?;
    if !heap.validate_value(&value) {
        return Err(RuntimeError::module_validation("invalid inspected value"));
    }
    let _borrow = StorageBorrow::enter(&heap.storage_borrows);
    inspect_loaned_value(heap, value, read)
}

/// Inspect a checked value while its caller owns storage exclusion and keeps
/// the traced source alive. Native loans reuse their existing exclusion guard.
pub(crate) fn inspect_loaned_value<R>(
    heap: &GcHeap,
    value: Value,
    read: impl for<'view> FnOnce(ValueView<'view>) -> NativeResult<R>,
) -> NativeResult<R> {
    assert!(heap.storage_borrows.get() > 0);
    read(ValueView {
        heap,
        value,
        invariant: PhantomData,
    })
}

fn type_error(expected: &str) -> RuntimeError {
    RuntimeError::module_validation(format!("inspection expects {expected}"))
}
