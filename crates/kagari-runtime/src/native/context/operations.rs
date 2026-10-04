//! Checked allocation, storage leases and selected call inspection for native libraries.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{HeapObjectId, custom_keys::KeyLookupGuard},
    native::{
        binding::NativeResult,
        context::{CallContext, LinkedCallable},
        sequence_edit::SequenceEdit,
    },
    value::{EnumTag, Value},
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::ty::Ty;
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
    /// Allocate a checked tagged value. Declared layouts, fields and generations
    /// are validated by the heap; dynamic native results are checked on return.
    pub fn enum_value(&self, tag: EnumTag, fields: Vec<Value>) -> NativeResult<Value> {
        self.runtime.validate_heap_payloads(&fields)?;
        self.heap().alloc_enum(tag, fields).map(Value::Enum)
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
