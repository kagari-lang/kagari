//! Checked allocation, storage leases and selected call inspection for native libraries.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{HeapObjectId, custom_keys::KeyLookupGuard},
    module::LoadedModule,
    native::{
        binding::NativeResult,
        context::{CallContext, LinkedCallable},
        sequence_edit::SequenceEdit,
    },
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::{collection::CollectionAccess, ty::Ty};
use std::slice;

/// A rooted synchronous key lookup; recursive structural mutation remains blocked.
pub struct NativeKeyLookupGuard<'call> {
    pub(super) _guard: KeyLookupGuard<'call>,
}

impl LinkedCallable {
    /// The checked intrinsic operation, when this selection has an intrinsic body.
    pub fn primitive(&self) -> Option<RuntimePrimitive> {
        self.primitive
    }

    /// Checked parameter facts for representation-specific native fast paths.
    pub fn parameters(&self) -> &[Ty<DefinitionId>] {
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

    /// Admit a call-scoped Vec receiver with its retained element contract and
    /// declared access. Storage methods still check dynamic leases and bounds.
    pub fn array_argument(&self, index: usize, writable: bool) -> NativeResult<HeapObjectId> {
        let signature = self.function.type_signature()?;
        let expected = signature
            .params
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("native array argument slot"))?;
        check_array_argument(self.runtime, self.owner, expected, writable, || {
            self.argument(index)
        })
    }

    /// Copy sequence storage while preserving its checked element contract.
    pub fn clone_sequence(&self, source: HeapObjectId) -> NativeResult<HeapObjectId> {
        self.heap().clone_array(source)
    }

    /// Check generation and active borrow/lease guards before a structural edit.
    pub fn ensure_collection_mutable(&self, target: HeapObjectId) -> NativeResult<()> {
        self.heap().ensure_structure_mutable(target)
    }

    /// Root traced elements, detach an exclusive buffer, and restore it on every
    /// success, trap or unwind. Callbacks may synchronously reenter the host.
    pub fn edit_sequence<R>(
        &mut self,
        target: HeapObjectId,
        edit: impl for<'buffer> FnOnce(&mut Self, SequenceEdit<'buffer>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let heap = self.heap();
        heap.edit_sequence(target, |buffer| edit(self, buffer))
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

/// Shared admission for rooted native arguments and closed primitive operands.
pub(crate) fn check_array_argument(
    runtime: &Runtime,
    owner: &LoadedModule,
    expected: &TypeArgument,
    writable: bool,
    argument: impl FnOnce() -> NativeResult<Value>,
) -> NativeResult<HeapObjectId> {
    let Ty::Array(_, access) = expected.ty() else {
        return Err(RuntimeError::module_validation(
            "native array argument type",
        ));
    };
    if writable && *access != CollectionAccess::Mutable {
        return Err(RuntimeError::module_validation(
            "collection view is read-only",
        ));
    }
    let value = argument()?;
    if !expected.matches(runtime, &value, owner) {
        return Err(RuntimeError::module_validation(
            "native argument differs from its declared type",
        ));
    }
    let Value::Array(id) = value else {
        return Err(RuntimeError::module_validation("native array argument"));
    };
    Ok(id)
}
