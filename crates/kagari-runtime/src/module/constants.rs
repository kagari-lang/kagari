//! Runtime-local constant values are edges of the installed module version.
use crate::{Runtime, error::RuntimeError, module::LoadedModule, value::Value};
use kagari_bytecode::instruction::{ConstantId, ConstantOperand};
use std::sync::OnceLock;

/// Single-assignment cells permit shared execution links without mutable store
/// borrows. The runtime remains exclusively driven and can move between threads.
/// This pool owns no program root: only its module record supplies traced edges.
#[derive(Debug)]
pub(crate) struct ConstantPool {
    values: Box<[OnceLock<Value>]>,
}

impl ConstantPool {
    pub(super) fn new(len: usize) -> Self {
        Self {
            values: (0..len).map(|_| OnceLock::new()).collect(),
        }
    }

    pub(crate) fn get(&self, constant: ConstantId) -> Option<Value> {
        self.values.get(constant.index())?.get().copied()
    }

    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = &Value> {
        self.values.iter().filter_map(OnceLock::get)
    }

    /// The caller admits and retains the owning program before entering. This
    /// operation may allocate, so no interpreter operand-bank borrow may cross it.
    pub(crate) fn materialize(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        constant: ConstantId,
    ) -> Result<Value, RuntimeError> {
        let cell = self
            .values
            .get(constant.index())
            .ok_or_else(|| RuntimeError::module_validation("constant index out of bounds"))?;
        if let Some(value) = cell.get() {
            return Ok(*value);
        }
        let source = owner
            .bytecode
            .constants
            .get(constant.index())
            .ok_or_else(|| RuntimeError::module_validation("constant index out of bounds"))?;
        // Allocation cannot collect or invoke user code. Publish the edge before
        // the next safepoint; failure leaves the cell empty and safe to retry.
        let value = match source {
            ConstantOperand::Unit => Value::Unit,
            ConstantOperand::Bool(value) => Value::Bool(*value),
            ConstantOperand::I32(value) => Value::I32(*value),
            ConstantOperand::I64(value) => Value::I64(*value),
            ConstantOperand::U64(value) => Value::U64(*value),
            ConstantOperand::F32(value) => Value::F32(*value),
            ConstantOperand::F64(value) => Value::F64(*value),
            ConstantOperand::Str(text) => runtime.gc.alloc_string(text.clone())?,
        };
        cell.set(value).map_err(|_| {
            runtime
                .resources()
                .quarantine("constant initialized during exclusive execution")
        })?;
        Ok(value)
    }
}

impl Runtime {
    /// Materialize a portable constant once. The owning module version retains it;
    /// callers root an escaped value independently before releasing that version.
    pub fn read_constant(
        &self,
        module: &LoadedModule,
        constant: ConstantId,
    ) -> Result<Value, RuntimeError> {
        self.validate_loaded_module(module)?;
        let records = self.modules.inner.try_borrow_mut().map_err(|_| {
            self.resources()
                .quarantine("constant storage borrowed across execution")
        })?;
        let record = records
            .resolve(module)
            .ok_or_else(|| RuntimeError::module_validation("constant owner has been released"))?;
        record.constants.materialize(self, module, constant)
    }
}
