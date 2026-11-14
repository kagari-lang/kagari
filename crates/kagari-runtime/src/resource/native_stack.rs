//! Native admissions bound Rust-stack recursion separately from VM frame storage.
#[cfg(test)]
mod tests;
use crate::{
    error::{RuntimeError, fault::EngineFault, reason::ErrorReason},
    resource::ResourceState,
};
#[cfg(all(not(target_family = "wasm"), not(miri)))]
use stacker::remaining_stack;

/// Linear admission; the surrounding Native context owns cleanup on unwind.
pub(crate) struct NativeStackMark {
    _private: (),
}

/// Leave room for Runtime error capture/cleanup before another arbitrary Rust
/// body. Host callback internals still own their own recursion/stack usage.
#[cfg(all(not(target_family = "wasm"), not(miri)))]
const MIN_REMAINING_BYTES: usize = 128 * 1024;

impl ResourceState {
    pub(crate) fn native_stack_active(&self) -> bool {
        self.native_depth.get() != 0
    }

    pub(crate) fn enter_native_stack(&self) -> Result<NativeStackMark, RuntimeError> {
        self.poll_execution()?;
        let depth = self.native_depth.get();
        let exhausted = depth >= self.limits.max_native_depth.get();
        #[cfg(all(not(target_family = "wasm"), not(miri)))]
        let exhausted =
            exhausted || remaining_stack().is_some_and(|remaining| remaining < MIN_REMAINING_BYTES);
        if exhausted {
            return Err(
                self.admit_execution_error(RuntimeError::new(ErrorReason::NativeStackOverflow))
            );
        }
        self.native_depth.set(depth + 1);
        Ok(NativeStackMark { _private: () })
    }

    pub(crate) fn leave_native_stack(&self, _mark: NativeStackMark) {
        if let Some(depth) = self.native_depth.get().checked_sub(1) {
            self.native_depth.set(depth);
        } else {
            self.quarantine(EngineFault::HostInvariant(
                "Native stack admission underflow",
            ));
        }
    }
}
