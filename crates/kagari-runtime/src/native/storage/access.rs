//! Loan-scoped validation, retention, and structural publication services.
use crate::{
    Runtime,
    error::RuntimeError,
    native::{binding::NativeResult, storage_type::StorageType, value_handle::ScriptValue},
    value::Value,
};

/// Preflight the largest final payload occupancy and a possible structural change.
/// Logical units exclude the identity and shallow Rust allocation.
#[derive(Debug, Clone, Copy)]
pub struct EditPlan {
    pub max_logical_units: usize,
    pub structural: bool,
}

/// Services for one non-reentrant native payload loan. No payload reference or
/// unrooted extracted edge may survive that loan. Recovery permits only revision
/// publication and republication of already admitted edges.
pub struct StorageAccess<'loan> {
    runtime: &'loan Runtime,
    revision: &'loan mut u64,
    next_revision: Option<u64>,
    recovery: bool,
}

impl<'loan> StorageAccess<'loan> {
    pub(crate) fn new(
        runtime: &'loan Runtime,
        revision: &'loan mut u64,
        next_revision: Option<u64>,
        recovery: bool,
    ) -> Self {
        Self {
            runtime,
            revision,
            next_revision,
            recovery,
        }
    }

    fn validate(&self, contract: &StorageType, value: &Value) -> NativeResult<()> {
        if self.recovery {
            return Err(RuntimeError::module_validation(
                "native recovery cannot retain or publish new values",
            ));
        }
        self.runtime.validate_executable_module(&contract.owner)?;
        contract.argument()?.validate(self.runtime)?;
        let heap = self.runtime.gc();
        if !value.is_default_heap_payload(heap)
            || !heap.validate_value(value)
            || !contract.accepts_value(heap, value)
        {
            return Err(RuntimeError::module_validation(
                "native storage value differs from its closed contract",
            ));
        }
        Ok(())
    }

    /// Retain a checked edge before it leaves this payload loan.
    pub fn retain(&mut self, contract: &StorageType, value: &Value) -> NativeResult<ScriptValue> {
        self.validate(contract, value)?;
        ScriptValue::retain_storage(self.runtime, contract, *value)
    }

    /// Check exact type, runtime, generation, and heap lifetime before storing an edge.
    /// Synchronous collection cannot run until this loan ends.
    pub fn publish(&mut self, contract: &StorageType, input: &ScriptValue) -> NativeResult<Value> {
        if self.recovery {
            return Err(RuntimeError::module_validation(
                "native recovery cannot publish new values",
            ));
        }
        let value = input.storage_value(self.runtime, contract)?;
        self.validate(contract, &value)?;
        Ok(value)
    }

    /// Republish an edge already admitted to this same protected payload.
    ///
    /// # Safety
    /// The edge must remain traced in the same payload throughout the operation;
    /// this cannot import an external or expired value. No new edge is admitted.
    pub unsafe fn republish_existing(&mut self, value: &Value) {
        // Identity validation is allocation-free and requires no execution admission.
        // Full synchronous scans see the final slot arrangement after the loan.
        if !value.is_default_heap_payload(self.runtime.gc())
            || !self.runtime.gc().validate_value(value)
        {
            self.runtime
                .resources()
                .quarantine("invalid existing native edge during republication");
        }
    }

    pub fn revision(&self) -> u64 {
        *self.revision
    }

    pub fn check_revision(&self, expected: u64) -> NativeResult<()> {
        if self.revision() != expected {
            return Err(RuntimeError::module_validation(
                "native structure changed during operation",
            ));
        }
        Ok(())
    }

    /// Publish immediately before an infallible structural write, after all
    /// fallible preparation. The edit must preflight this transition.
    ///
    /// # Panics
    /// Panics if the provider did not request a structural edit. Repeated calls
    /// within the same structural loan have no additional effect.
    pub fn commit_structure(&mut self) {
        *self.revision = self.next_revision.expect("structural edit was preflighted");
    }
}
