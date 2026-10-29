//! Branded value transport backed by an active HostScope's existing roots.
use crate::{
    error::RuntimeError,
    gc::{
        GcHeap,
        inspection::{ValueView, inspect_value},
        roots::RootedValue,
    },
    native::binding::NativeResult,
    value::Value,
};
use std::{fmt, marker::PhantomData, ptr};

/// One value protected by its originating host scope.
///
/// Copies share that scope's protection, never expose an unrooted managed value,
/// and cannot outlive the invariant scope brand. Use HostScope::scalar or retain,
/// or a borrowed view's argument method, to construct one. Runtime ownership is
/// checked before transport is admitted to execution.
///
/// ```compile_fail
/// use kagari_runtime::{Runtime, native::rust_type::RustType};
/// fn escape(runtime: &mut Runtime, ty: &RustType<i32>, value: &i32) {
///     let argument = runtime.with_host_scope(|scope| {
///         Ok(scope.borrow(ty, value)?.argument())
///     }).unwrap();
///     drop(argument);
/// }
/// ```
#[derive(Clone, Copy)]
pub struct ScopedValue<'scope> {
    heap: &'scope GcHeap,
    value: Value,
    brand: PhantomData<&'scope mut &'scope ()>,
}

impl<'scope> ScopedValue<'scope> {
    /// Inspect a protected value through the shared non-reentrant storage loan.
    /// Scalar copies and borrowed strings need no additional root registration.
    pub fn inspect<R>(
        &self,
        read: impl for<'view> FnOnce(ValueView<'view>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        inspect_value(self.heap, self.value, read)
    }

    /// Retain an ordinary script value independently of its host scope.
    /// External host roots/path views and borrowed capabilities remain scoped and
    /// are rejected; their Rust ownership is not transferred into the script heap.
    pub fn retain(&self) -> NativeResult<RootedValue> {
        self.heap.root_value(self.value).ok_or_else(|| {
            RuntimeError::host_borrow_escape("scoped value cannot become an owning script root")
        })
    }

    pub(super) fn new(heap: &'scope GcHeap, value: Value) -> Self {
        Self {
            heap,
            value,
            brand: PhantomData,
        }
    }

    pub(super) fn value(&self, heap: &GcHeap) -> NativeResult<Value> {
        if !ptr::eq(self.heap, heap) {
            return Err(RuntimeError::module_validation("foreign scoped argument"));
        }
        Ok(self.value)
    }
}

impl fmt::Debug for ScopedValue<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScopedValue")
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}
