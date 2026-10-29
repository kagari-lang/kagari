//! Debug snapshots retain values independently of the paused execution frame.
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    gc::{
        inspection::ValueView,
        roots::{RootSet, RootedValue},
    },
    native::binding::NativeResult,
};

/// A retained debugger value. Cloning shares its immutable snapshot lease.
///
/// Dropping the pause or resuming execution does not release this value while
/// a clone remains. Scoped host capabilities still obey their original scope;
/// retaining the debugger snapshot does not retain borrowed Rust state.
#[derive(Debug, Clone, PartialEq)]
pub struct DebugValue {
    roots: RootSet,
}

impl DebugValue {
    pub(super) fn new(roots: RootSet) -> Self {
        Self { roots }
    }

    /// Read copied scalars or borrowed string bytes under a non-reentrant loan.
    /// Foreign runtimes, expired snapshots and unavailable storage reject.
    pub fn inspect<R>(
        &self,
        runtime: &Runtime,
        read: impl for<'view> FnOnce(ValueView<'view>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.roots.inspect(runtime, 0, read)
    }

    /// Retain an ordinary script value for typed host conversion.
    ///
    /// The new root is independent of this debug snapshot. Scoped host resources
    /// reject because an owning script root cannot extend their borrow lifetime.
    pub fn retain(&self, runtime: &Runtime) -> NativeResult<RootedValue> {
        // SAFETY: The private snapshot lease cannot be replaced by callers.
        // No callback or collection separates extraction and root publication.
        let value = unsafe { self.roots.get_transport(runtime.gc(), 0) }
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired debug snapshot"))?;
        unsafe { runtime.gc().root_value_transport(value) }.ok_or_else(|| {
            RuntimeError::module_validation("debug value cannot become an owning script root")
        })
    }
}
