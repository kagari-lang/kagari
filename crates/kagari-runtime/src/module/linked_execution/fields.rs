//! Physical field operands share the exact function/environment lifetime.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::fields::prepared::PreparedStructField,
    module::{StructLayoutRef, linked_execution::LinkedFunction},
};
use std::sync::OnceLock;

#[derive(Debug)]
pub(super) struct LinkedFields {
    entries: Box<[OnceLock<PreparedStructField>]>,
}

impl LinkedFields {
    pub(super) fn new(count: usize) -> Self {
        Self {
            entries: (0..count).map(|_| OnceLock::new()).collect(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

impl LinkedFunction {
    pub(crate) fn ready_field(&self, index: usize) -> Option<&PreparedStructField> {
        self.fields.entries.get(index)?.get()
    }

    /// Preparation runs at the original instruction, outside operand-bank loans.
    /// Applied functions use fresh cells for each exact environment. Host-reference
    /// fields retain their scoped owner and never enter this script storage path.
    pub(crate) fn prepare_field(
        &self,
        runtime: &Runtime,
        index: usize,
        layout: StructLayoutRef,
        slot: usize,
    ) -> Result<(), RuntimeError> {
        let entry = self
            .fields
            .entries
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("invalid linked field ordinal"))?;
        if entry.get().is_none() {
            let prepared = runtime.gc().prepare_struct_field(layout, slot)?;
            let _ = entry.set(prepared);
        }
        Ok(())
    }
}
