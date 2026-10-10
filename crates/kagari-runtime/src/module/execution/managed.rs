//! Closed managed operations retain physical locations from verified code.
use crate::module::execution::{OperandSlot, layout::Location};
use kagari_bytecode::instruction::ConstantId;

/// Only preparation can construct operations consumed by the admitted cursor.
#[derive(Debug, Clone, Copy)]
pub struct PreparedManagedOperation(pub(crate) ManagedOperation);

#[derive(Debug, Clone, Copy)]
pub(crate) enum ManagedOperation {
    Constant { dst: Location, constant: ConstantId },
    Copy { dst: Location, src: Location },
    Return { value: OperandSlot },
    Field { index: usize },
    Index { index: usize },
}
