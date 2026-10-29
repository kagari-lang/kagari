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
        let Some(field) = self.links.and_then(|links| links.ready_field(index)) else {
            return Ok(FieldContinuation::Prepare);
        };
        let layout = field.layout();
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
                    // SAFETY: admitted operands belong to this runtime and the
                    // closed cursor cannot collect or invoke callbacks before publication.
                    let bits = unsafe { field.read_scalar(self.runtime.gc(), id, ty) }
                        .ok_or(RegionError::TypeMismatch("struct layout or field mismatch"))?;
                    self.write_payload(dst.operand.scalar().ok_or_else(|| self.invalid())?, bits)
                        .ok_or_else(|| self.invalid())?;
                }
                FieldAccess::Write { .. } => {
                    // SAFETY: admitted receiver remains rooted through the closed
                    // scalar commit, which cannot collect or invoke a callback.
                    unsafe {
                        field.write_scalar(
                            self.runtime.gc(),
                            id,
                            ty,
                            bits.expect("prepared scalar write"),
                        )
                    }?;
                }
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
        let result = match (action, base) {
            (FieldAction::Read, Value::Struct(reference)) => {
                // SAFETY: the traced receiver and operand-bank loan remain live
                // through this read and publication, without reentry or collection.
                Some(
                    unsafe { field.read(self.runtime.gc(), reference) }
                        .ok_or(RegionError::TypeMismatch("struct layout or field mismatch"))?,
                )
            }
            (FieldAction::Write(value), Value::Struct(reference)) => {
                // SAFETY: both admitted operands remain protected through barrier
                // publication and commit, with no intervening safepoint.
                unsafe { field.write(self.runtime.gc(), reference, value) }?;
                None
            }
            (action, base) => {
                action.execute(self.runtime, layout, operation.slot as usize, base)?
            }
        };
        if let Some(value) = result {
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
