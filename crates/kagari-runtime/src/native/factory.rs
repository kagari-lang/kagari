//! An entry's execution policy, independent of its script declaration identity.
use crate::{
    RuntimeError,
    native::{NativeContext, NativeEntry, NativeInvocationState},
};
use std::rc::Rc;

/// Low-level rooted/resumable entry factory. Creating a descriptor does not invoke the entry.
pub struct NativeFactory {
    pub(crate) scratch_slots: usize,
    pub(crate) entry: Rc<NativeEntry>,
}

impl NativeFactory {
    pub fn new(
        scratch_slots: usize,
        entry: impl Fn(&mut NativeContext<'_>) -> Result<Box<dyn NativeInvocationState>, RuntimeError>
        + 'static,
    ) -> Self {
        Self {
            scratch_slots,
            entry: Rc::new(entry),
        }
    }
}
