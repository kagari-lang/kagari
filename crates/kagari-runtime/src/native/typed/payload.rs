//! Immediate provider access validates a value without creating retention.
#[cfg(test)]
mod tests;

use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        payload::{borrowed::admit_payload_mapping, read::ReadReceiver},
        storage::{NativePayload, access::StorageAccess},
        storage_type::StorageType,
        typed::NativeContext,
    },
    value::Value,
};

impl NativeContext<'_> {
    /// Read a checked native payload immediately, without retaining its value.
    ///
    /// The contract preserves the value's lexical type owner; this context supplies
    /// live execution authority. Every operation checks runtime, generation, kind,
    /// nominal application, installed Rust payload mapping and graph admission.
    /// Admission invokes no user conversion or collection. The read uses the same
    /// non-reentrant loan as owning and argument-backed payload access.
    ///
    /// Invalid or unavailable values, execution/mapping failures, cancellation and
    /// conflicting loans or operation guards return an error before `read` runs.
    /// Contract/value admission precedes executable/mapping admission. No root,
    /// borrowed handle or reusable access authority is returned. Errors and unwinds
    /// restore the loan before propagating; a read does not run provider charging.
    ///
    /// # Safety
    /// Keep the value protected by a root or an unchanged traced edge until this
    /// operation begins. A copied `Value` is not a root: retain it before an
    /// intervening callback, collection or mutation can remove that protection.
    /// During `read`, collection, take, competing loans and script reentry are
    /// excluded. Interior mutation must not change edges, structure or accounting.
    /// Although `R` cannot borrow the payload, it can contain copied values; retain
    /// those before their source protection expires or changes.
    ///
    /// ```compile_fail
    /// use kagari_runtime::{
    ///     native::{binding::NativeResult, storage_type::StorageType, typed::NativeContext},
    ///     value::Value,
    /// };
    /// fn escape<'a>(
    ///     cx: &'a NativeContext<'_>, value: Value, contract: &StorageType,
    /// ) -> NativeResult<&'a u32> {
    ///     unsafe { cx.read_payload::<u32, _>(value, contract, |payload, _| Ok(payload)) }
    /// }
    /// ```
    pub unsafe fn read_payload<P: NativePayload, R>(
        &self,
        value: Value,
        contract: &StorageType,
        read: impl for<'loan> FnOnce(&'loan P, &mut StorageAccess<'loan>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let runtime = self.runtime();
        let heap = runtime.gc();
        heap.ensure_no_native_borrow()?;
        // Preserve source retention's type/publication ordering, but not its root.
        // Consuming the checked target releases its directory borrow before access.
        contract.checked_target(runtime, &value)?.publish()?;
        let expected = contract.argument()?;
        admit_payload_mapping::<P>(self, expected)?;
        let Value::GcHandle(id) = value else {
            return Err(RuntimeError::module_validation("native storage value"));
        };
        heap.check_native_payload_argument::<P>(id, expected, &contract.owner)?;
        heap.read_native(runtime, ReadReceiver::Value(id), None, read)
    }
}
