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
};

impl ExecutionCursor<'_> {
    #[inline(never)]
    pub(super) fn execute_field(
        &mut self,
        index: usize,
    ) -> Result<Option<CursorExit>, RegionError> {
        let operation = self
            .frame
            .prepared_field(index)
            .ok_or_else(|| self.invalid())?;
        let Some(layout) = operation
            .concrete_layout(self.frame.loaded())
            .or_else(|| self.frame.ready_field_layout(operation.pc).cloned())
        else {
            return Ok(Some(CursorExit::Transition(PreparedTransition::Field {
                index,
            })));
        };
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
        if let Some(value) = action.execute(self.runtime, &layout, operation.slot as usize, base)? {
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
