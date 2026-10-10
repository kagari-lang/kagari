//! Closed managed operations retain physical locations from verified code.
use crate::module::execution::{OperandSlot, PreparedField, layout::Location};
use kagari_bytecode::instruction::Register;

/// Only preparation can construct operations consumed by the admitted cursor.
#[derive(Debug, Clone, Copy)]
pub struct PreparedManagedOperation(pub(crate) ManagedOperation);

#[derive(Debug, Clone, Copy)]
pub(crate) enum ManagedOperation {
    Copy {
        dst: Location,
        src: Location,
    },
    Return {
        value: OperandSlot,
    },
    ReadField {
        dst: Register,
        base: Register,
        field: PreparedField,
    },
    WriteField {
        base: Register,
        value: Register,
        field: PreparedField,
    },
}
