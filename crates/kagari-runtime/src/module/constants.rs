//! Runtime-local constant values are edges of the installed module version.
use crate::{Runtime, error::RuntimeError, module::LoadedModule, value::Value};
use kagari_bytecode::instruction::{ConstantId, ConstantOperand};

impl Runtime {
    /// Materialize a portable constant once. The owning module version retains it;
    /// callers root an escaped value independently before releasing that version.
    pub fn read_constant(
        &self,
        module: &LoadedModule,
        constant: ConstantId,
    ) -> Result<Value, RuntimeError> {
        self.validate_loaded_module(module)?;
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            self.resources()
                .quarantine("constant storage borrowed across execution")
        })?;
        let record = records
            .resolve_mut(module)
            .ok_or_else(|| RuntimeError::module_validation("constant owner has been released"))?;
        let cached = record
            .constants
            .get_mut(constant.index())
            .ok_or_else(|| RuntimeError::module_validation("constant index out of bounds"))?;
        if let Some(value) = cached {
            return Ok(*value);
        }
        // Heap allocation cannot collect or call user code. The record receives
        // its edge before any subsequent safepoint can inspect the program graph.
        let value = match &module.bytecode.constants[constant.index()] {
            ConstantOperand::Unit => Value::Unit,
            ConstantOperand::Bool(value) => Value::Bool(*value),
            ConstantOperand::I32(value) => Value::I32(*value),
            ConstantOperand::I64(value) => Value::I64(*value),
            ConstantOperand::U64(value) => Value::U64(*value),
            ConstantOperand::F32(value) => Value::F32(*value),
            ConstantOperand::F64(value) => Value::F64(*value),
            ConstantOperand::Str(text) => self.gc.alloc_string(text.clone())?,
        };
        *cached = Some(value);
        Ok(value)
    }
}
