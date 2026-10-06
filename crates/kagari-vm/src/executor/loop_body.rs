//! Run non-reentrant operations with one frame/window borrow. Slow boundaries
//! publish the logical PC and release all borrows before entering the runtime.
use crate::{error::VmError, executor::Executor};
use kagari_runtime::frame::{cursor::kernel::RegionExit, transfer::ReturnValue};

pub(super) enum LoopExit {
    Safepoint,
    Boundary,
    Return(ReturnValue),
}

impl Executor<'_> {
    pub(super) fn run_cursor(&self) -> Result<LoopExit, VmError> {
        let mut frame = self.stack.cursor(self.runtime)?;
        Ok(match frame.execute_region()? {
            RegionExit::Safepoint => LoopExit::Safepoint,
            RegionExit::Return(value) => LoopExit::Return(value),
            RegionExit::Boundary => LoopExit::Boundary,
        })
    }
}
