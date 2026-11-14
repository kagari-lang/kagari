//! Closed sequence access uses the same physical providers as Native adapters.
use crate::{
    error::{RuntimeError, trap::TrapReason},
    frame::{
        ExecutionFrame,
        cursor::{
            ExecutionCursor,
            kernel::{CursorExit, PreparedTransition, RegionError},
        },
    },
    module::execution::indices::{IndexAccess, PreparedIndexOperation},
    value::{
        aggregates::tuples::TupleObject, arrays::ArrayObject, kind::ObjectKind,
        properties::ValueProperties,
    },
    value_check::{data::candidate_access, scoped},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::module::CallableTarget;
use kagari_gc::error::GcError;

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

impl ExecutionCursor<'_, '_, '_, '_> {
    #[inline(never)]
    pub(super) fn execute_index(
        &mut self,
        ordinal: usize,
    ) -> Result<Option<CursorExit>, RegionError> {
        let operation = *self
            .function
            .indices
            .get(ordinal)
            .ok_or_else(|| self.invalid())?;
        // Keep base, index and value reads in their original left-to-right order.
        let base = self
            .values
            .read_location(operation.base)
            .map_err(RegionError::storage)?;
        let index_value = self
            .values
            .read_location(operation.index)
            .map_err(RegionError::storage)?;
        let value = match operation.access {
            IndexAccess::Read { .. } => None,
            IndexAccess::Write { value } => Some(
                self.values
                    .read_location(value)
                    .map_err(RegionError::storage)?,
            ),
        };
        let index = match index_value.scalar_payload() {
            Some((ValueType::I32 | ValueType::I64, bits)) if bits as i64 >= 0 => bits,
            Some((ValueType::U64, bits)) => bits,
            _ => {
                return Err(RegionError::TypeMismatch(match operation.access {
                    IndexAccess::Read { .. } => "read_index expects non-negative integer index",
                    IndexAccess::Write { .. } => "write_index expects non-negative integer index",
                }));
            }
        };
        match operation.access {
            IndexAccess::Read { dst } => {
                let object = base.object().ok_or(RegionError::TypeMismatch(
                    "read_index expects array or tuple value",
                ))?;
                let cx = self.values.scope();
                candidate_access(cx, base, self.resources).map_err(RegionError::from)?;
                let value = match base.kind() {
                    Some(ObjectKind::Array) => {
                        let array = cx
                            .downcast::<ArrayObject>(object)
                            .map_err(RegionError::storage)?;
                        ArrayObject::get(
                            cx,
                            array,
                            usize::try_from(index).map_err(|_| RegionError::InvalidIndex(index))?,
                        )
                    }
                    Some(ObjectKind::Tuple) => {
                        let tuple = cx
                            .downcast::<TupleObject>(object)
                            .map_err(RegionError::storage)?;
                        TupleObject::get(
                            cx,
                            tuple,
                            usize::try_from(index).map_err(|_| RegionError::InvalidIndex(index))?,
                        )
                    }
                    _ => {
                        return Err(RegionError::TypeMismatch(
                            "read_index expects array or tuple value",
                        ));
                    }
                }
                .map_err(|error| index_error(error, index))?;
                self.values
                    .write_location(dst, value)
                    .map_err(RegionError::storage)?;
            }
            IndexAccess::Write { .. } => {
                let value = value.ok_or_else(|| self.invalid())?;
                match base.kind() {
                    Some(ObjectKind::Array) => {
                        let cx = self.values.scope();
                        if !ValueProperties::of(cx, value)
                            .map_err(RegionError::storage)?
                            .storable
                        {
                            return Err(RegionError::TypeMismatch(
                                "write_index expects default-storable value",
                            ));
                        }
                        candidate_access(cx, value, self.resources).map_err(RegionError::from)?;
                        candidate_access(cx, base, self.resources).map_err(RegionError::from)?;
                        let array = cx
                            .downcast::<ArrayObject>(base.object().ok_or({
                                RegionError::TypeMismatch(
                                    "write_index expects array or tuple value",
                                )
                            })?)
                            .map_err(RegionError::storage)?;
                        if !cx
                            .read(array, |array| {
                                scoped::matches(
                                    cx,
                                    value,
                                    array.contract().view(),
                                    self.host,
                                    self.host_borrows,
                                    self.resources,
                                )
                            })
                            .map_err(RegionError::from)?
                        {
                            return Err(RuntimeError::from(
                                TrapReason::InvalidSequenceBufferOrPayload,
                            )
                            .into());
                        }
                        ArrayObject::set(
                            cx,
                            array,
                            usize::try_from(index).map_err(|_| RegionError::InvalidIndex(index))?,
                            value,
                        )
                        .map_err(|error| index_error(error, index))?;
                    }
                    Some(ObjectKind::Tuple) => {
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

fn index_error(error: GcError, index: u64) -> RegionError {
    match error {
        GcError::Index => RegionError::InvalidIndex(index),
        error => RegionError::storage(error),
    }
}
