//! Ready layouts execute the same physical field operation as cold preparation.
use crate::frame::{
    cursor::{ExecutionCursor, kernel::RegionError},
    fields::access,
};

pub(super) enum FieldContinuation {
    Complete,
    Prepare,
}

impl ExecutionCursor<'_, '_, '_, '_> {
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
        access::execute(
            &mut self.values,
            operation,
            layout,
            self.host,
            self.host_borrows,
            self.resources,
        )?;
        Ok(FieldContinuation::Complete)
    }
}
