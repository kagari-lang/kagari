//! Admitted concrete and scoped fields share physical operands and storage kernels.
use crate::{
    frame::{
        cursor::{
            ExecutionCursor,
            kernel::{CursorExit, PreparedTransition, RegionError},
        },
        fields::FieldAction,
    },
    module::execution::fields::FieldAccess,
    value::Value,
};

impl ExecutionCursor<'_> {
    #[inline(never)]
    pub(super) fn execute_field(
        &mut self,
        index: usize,
    ) -> Result<Option<CursorExit>, RegionError> {
        let operation = self
            .function
            .fields
            .get(index)
            .ok_or_else(|| self.invalid())?;
        if operation.reference.is_some() {
            return Ok(Some(CursorExit::Transition(PreparedTransition::Field {
                index,
            })));
        }
        let Some(layout) = self
            .links
            .and_then(|links| links.ready_field_layout(operation.pc))
        else {
            return Ok(Some(CursorExit::Transition(PreparedTransition::Field {
                index,
            })));
        };
        if let Some(ty) = operation.scalar {
            let bits = match operation.access {
                FieldAccess::Read { .. } => None,
                FieldAccess::Write { value } => {
                    Some(self.payload(value.operand.scalar().ok_or_else(|| self.invalid())?)?)
                }
            };
            let base = self
                .values
                .read_location(operation.base)
                .ok_or_else(|| self.invalid())?;
            let Value::Struct(id) = base else {
                return Err(RegionError::TypeMismatch(match operation.access {
                    FieldAccess::Read { .. } => "read_field expects struct value",
                    FieldAccess::Write { .. } => "write_field expects struct value",
                }));
            };
            match operation.access {
                FieldAccess::Read { dst } => {
                    let bits = self
                        .runtime
                        .gc()
                        .struct_scalar(id, layout, operation.slot as usize, ty)
                        .ok_or(RegionError::TypeMismatch("struct layout or field mismatch"))?;
                    self.write_payload(dst.operand.scalar().ok_or_else(|| self.invalid())?, bits)
                        .ok_or_else(|| self.invalid())?;
                }
                FieldAccess::Write { .. } => self.runtime.gc().set_struct_scalar(
                    id,
                    layout,
                    operation.slot as usize,
                    ty,
                    bits.expect("prepared scalar write"),
                )?,
            }
            return Ok(None);
        }
        let action = match operation.access {
            FieldAccess::Read { .. } => FieldAction::Read,
            FieldAccess::Write { value } => FieldAction::Write(
                self.values
                    .read_location(value)
                    .ok_or_else(|| self.invalid())?,
            ),
        };
        action.validate(self.runtime)?;
        let base = self
            .values
            .read_location(operation.base)
            .ok_or_else(|| self.invalid())?;
        if let Some(value) = action.execute(self.runtime, layout, operation.slot as usize, base)? {
            let FieldAccess::Read { dst } = operation.access else {
                return Err(self.invalid().into());
            };
            if !self.runtime.gc().validate_value(&value) {
                return Err(self.invalid().into());
            }
            self.values
                .write_location(dst, value)
                .ok_or_else(|| self.invalid())?;
        }
        Ok(None)
    }
}
