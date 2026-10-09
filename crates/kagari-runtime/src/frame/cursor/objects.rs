//! Concrete fields reuse sealed layouts within a nonallocating execution region.
use crate::{
    error::RuntimeError, frame::cursor::ExecutionCursor, module::execution::PreparedField,
    value::Value,
};
use kagari_bytecode::instruction::Register;

impl ExecutionCursor<'_> {
    /// An invalid receiver falls back before any write, preserving VM diagnostics.
    #[inline(never)]
    pub(super) fn read_field(
        &mut self,
        dst: Register,
        base: Register,
        field: PreparedField,
    ) -> Result<bool, RuntimeError> {
        let Value::Struct(id) = self.read_register(base)? else {
            return Ok(false);
        };
        let layout = field.layout(self.frame.loaded());
        let Some(value) = self
            .runtime
            .gc()
            .struct_get_slot(id, &layout, field.slot as usize)
        else {
            return Ok(false);
        };
        if !value.has_representation(field.representation) {
            return Ok(false);
        }
        self.write_register(dst, value)?;
        Ok(true)
    }

    #[inline(never)]
    pub(super) fn write_field(
        &self,
        base: Register,
        value: Register,
        field: PreparedField,
    ) -> Result<bool, RuntimeError> {
        let value = self.read_register(value)?;
        if !value.is_default_heap_payload(self.runtime.gc()) {
            return Ok(false);
        }
        let Value::Struct(id) = self.read_register(base)? else {
            return Ok(false);
        };
        let layout = field.layout(self.frame.loaded());
        self.runtime
            .gc()
            .struct_set_slot(id, &layout, field.slot as usize, value)?;
        Ok(true)
    }
}
