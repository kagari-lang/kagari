//! Index access shares the checked heap kernels used by native sequence adapters.
use crate::{
    error::RuntimeErrorKind,
    frame::{
        ExecutionFrame,
        cursor::{
            ExecutionCursor,
            kernel::{CursorExit, PreparedTransition, RegionError},
        },
    },
    module::execution::indices::{IndexAccess, PreparedIndexOperation},
    value::Value,
};
use kagari_bytecode::module::CallableTarget;

impl ExecutionFrame {
    pub(super) fn prepared_index(&self, index: usize) -> Option<&PreparedIndexOperation> {
        let CallableTarget::Script(function) = self.target else {
            return None;
        };
        self.loaded
            .execution()
            .functions
            .get(function.index())?
            .indices
            .get(index)
    }
}

impl ExecutionCursor<'_> {
    #[inline(never)]
    pub(super) fn execute_index(
        &mut self,
        ordinal: usize,
    ) -> Result<Option<CursorExit>, RegionError> {
        let operation = *self
            .frame
            .prepared_index(ordinal)
            .ok_or_else(|| self.invalid())?;
        // Preserve base, index, value evaluation order before interpreting the
        // index. Locations are sealed, but current heap identities remain checked.
        let read = |location| {
            self.values
                .read_location(location)
                .ok_or_else(|| self.invalid())
        };
        let base = read(operation.base)?;
        let index = read(operation.index)?;
        let value = match operation.access {
            IndexAccess::Read { .. } => None,
            IndexAccess::Write { value } => Some(read(value)?),
        };
        let index = match index {
            Value::I32(index) if index >= 0 => index as usize,
            Value::I64(index) if index >= 0 => index as usize,
            Value::U64(index) => index as usize,
            _ => {
                return Err(RegionError::TypeMismatch(match operation.access {
                    IndexAccess::Read { .. } => "read_index expects non-negative integer index",
                    IndexAccess::Write { .. } => "write_index expects non-negative integer index",
                }));
            }
        };
        match operation.access {
            IndexAccess::Read { dst } => {
                let value = match base {
                    Value::Array(id) => self.runtime.gc().array_get(id, index),
                    Value::Tuple(id) => self
                        .runtime
                        .gc()
                        .tuple(id)
                        .and_then(|values| values.get(index).copied()),
                    _ => {
                        return Err(RegionError::TypeMismatch(
                            "read_index expects array or tuple value",
                        ));
                    }
                }
                .ok_or(RegionError::InvalidIndex(index))?;
                if !self.runtime.gc().validate_value(&value) {
                    return Err(self.invalid().into());
                }
                self.values
                    .write_location(dst, value)
                    .ok_or_else(|| self.invalid())?;
            }
            IndexAccess::Write { .. } => {
                let value = value.ok_or_else(|| self.invalid())?;
                match base {
                    Value::Array(id) => {
                        if !value.is_default_heap_payload(self.runtime.gc()) {
                            return Err(RegionError::TypeMismatch(
                                "write_index expects default-storable value",
                            ));
                        }
                        // This kernel validates current access/element type/bounds.
                        // Replacement cannot grow the script heap or invoke user
                        // code; type checks may allocate temporary Rust metadata.
                        self.runtime
                            .gc()
                            .array_set(id, index, value)
                            .map_err(|error| {
                                if error.kind() == RuntimeErrorKind::IndexOutOfBounds {
                                    RegionError::InvalidIndex(index)
                                } else {
                                    error.into()
                                }
                            })?;
                    }
                    Value::Tuple(_) => {
                        return Ok(Some(CursorExit::Transition(
                            PreparedTransition::TupleWrite {
                                operation: ordinal,
                                index,
                            },
                        )));
                    }
                    _ => {
                        return Err(RegionError::TypeMismatch(
                            "write_index expects array or tuple value",
                        ));
                    }
                }
            }
        }
        Ok(None)
    }
}
