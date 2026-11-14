//! Checked allocation, storage leases and selected call inspection for native libraries.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        context::{LinkedCallable, NativeCall},
    },
    value::view::ValueView,
    value_check,
};
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::ty::Ty;
use std::slice;

impl LinkedCallable {
    /// The checked intrinsic operation, when this selection has an intrinsic body.
    pub fn primitive(&self) -> Option<RuntimePrimitive> {
        self.primitive
    }

    /// Checked parameter facts for representation-specific native fast paths.
    pub fn parameters(&self) -> &[Ty] {
        &self.params
    }
}

impl<'call> NativeCall<'call> {
    /// Read an existing argument root with its prepared closed type. This does
    /// not create an owning Rust handle or convert the value through host storage.
    pub fn checked_argument(&mut self, index: usize) -> NativeResult<ValueView<'call>> {
        let value = self.argument(index)?;
        let function = self.function;
        let expected = function
            .type_signature()?
            .params
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("native argument type"))?;
        if !value_check::matches_protected(self.runtime, self.owner, expected, &value)? {
            return Err(RuntimeError::module_validation(
                "native argument differs from its declared type",
            ));
        }
        Ok(value)
    }

    /// Resolve a selected result parameter in the selected generation's scope.
    pub fn selected_result_parameter(
        &self,
        target: &LinkedCallable,
        index: usize,
    ) -> NativeResult<TypeArgument> {
        let owner = target.owner(self.owner)?;
        let result = match target.scoped_signature.get() {
            Some(signature) => signature.result.clone(),
            None => self
                .runtime
                .resolve_type_arguments(&owner, slice::from_ref(&target.result))?
                .pop()
                .ok_or_else(|| RuntimeError::module_validation("selected native result type"))?,
        };
        result.parameter(self.runtime, &owner, index)
    }
}
