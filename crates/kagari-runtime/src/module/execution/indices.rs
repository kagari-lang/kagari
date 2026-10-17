//! Aggregate indexing retains physical operands outside the compact instruction stream.
use crate::module::execution::layout::Location;

#[derive(Debug, Clone, Copy)]
pub(crate) enum IndexAccess {
    Read { dst: Location },
    Write { value: Location },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PreparedIndexOperation {
    pub(crate) base: Location,
    pub(crate) index: Location,
    pub(crate) access: IndexAccess,
}
