//! Iterator advancement and declared result construction share one commit boundary.
use crate::{
    native::{binding::NativeResult, context::CallContext, types::VariantRef},
    value::Value,
};
use kagari_contract::operations::IterOp;

impl CallContext<'_> {
    /// Apply a checked built-in operation independently of this call's result type.
    pub fn iter_operation(&self, index: usize, op: IterOp) -> NativeResult<Value> {
        self.runtime.iter_operation_with_type(
            self.owner,
            &self.argument(index)?,
            &self.argument_type_argument(index)?,
            op,
        )
    }

    /// Publish an iterator item through this call's prepared enum result contract.
    /// The present member receives one payload; the empty member receives none.
    /// Validate/allocate the result before committing cursor state, without reentry.
    pub fn iterator_next_result(
        &self,
        index: usize,
        present: &VariantRef,
        empty: &VariantRef,
    ) -> NativeResult<Value> {
        let value = self.argument(index)?;
        let source = self.argument_type_argument(index)?;
        self.runtime.validate_loaded_module(self.owner)?;
        source.validate(self.runtime)?;
        self.runtime
            .check_iterator_contract(self.owner, &value, &source)?;
        let result = self.result_type_argument()?;
        self.heap()
            .advance_iter_with(&value, source.ty(), |payload| {
                self.allocate_enum(
                    &result,
                    if payload.is_some() { present } else { empty },
                    payload.into_iter().collect(),
                )
            })
    }
}
