//! Admitted concrete and scoped fields share physical operands and storage kernels.
use crate::{
    frame::{
        cursor::{ExecutionCursor, kernel::RegionError},
        fields::FieldAction,
    },
    module::execution::fields::FieldAccess,
    value::Value,
};

pub(super) enum FieldContinuation {
    Complete,
    Prepare,
}

impl ExecutionCursor<'_> {
    #[inline(never)]
    pub(super) fn execute_field(&mut self, index: usize) -> Result<FieldContinuation, RegionError> {
        let operation = self
            .function
            .fields
            .get(index)
            .ok_or_else(|| self.invalid())?;
        if operation.reference.is_some() {
            return Ok(FieldContinuation::Prepare);
        }
        let Some(layout) = self
            .links
            .and_then(|links| links.ready_field_layout(operation.layout_site))
        else {
            return Ok(FieldContinuation::Prepare);
        };
        if let Some(ty) = operation.scalar {
            let bits = match operation.access {
                FieldAccess::Read { .. } => None,
                FieldAccess::Write { value } => Some(
                    self.payload(value.operand.scalar().ok_or_else(|| self.invalid())?)
                        .ok_or_else(|| self.invalid())?,
                ),
            };
            let id = self
                .values
                .with_value(operation.base, |value| match value {
                    Value::Struct(id) => Some(*id),
                    _ => None,
                })
                .ok_or_else(|| self.invalid())?;
            let Some(id) = id else {
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
            return Ok(FieldContinuation::Complete);
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
            self.values
                .write_location(dst, value)
                .ok_or_else(|| self.invalid())?;
        }
        Ok(FieldContinuation::Complete)
    }
}
