//! Checked allocation, storage leases and selected call inspection for native libraries.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        context::{CallContext, LinkedCallable},
    },
    value::Value,
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

impl<'call> CallContext<'call> {
    /// Read an existing argument root with its prepared closed type. This does
    /// not create an owning Rust handle or convert the value through host storage.
    pub fn checked_argument(&self, index: usize) -> NativeResult<Value> {
        let value = self.argument(index)?;
        let signature = self.function.type_signature()?;
        if !signature
            .params
            .get(index)
            .is_some_and(|ty| ty.matches(self.runtime, &value, self.owner))
        {
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
        let result = match &target.scoped_signature {
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
