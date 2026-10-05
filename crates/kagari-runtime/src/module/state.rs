//! Checked module-state access and the storage boundary for slot replacement.
use crate::{
    Runtime,
    error::RuntimeError,
    module::{LoadedModule, ModuleInstance, ModuleStore},
    value::Value,
};
use kagari_bytecode::{instruction::ModuleSlot, module::BytecodeModuleSlot};
use std::{mem, slice};

impl Runtime {
    fn validate_module_state_access(&self, module: &LoadedModule) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        self.validate_loaded_module(module)?;
        if !self.allows_instance_access(module.key()) {
            return Err(RuntimeError::execution_phase_violation(
                "candidate cannot access external module state",
            ));
        }
        Ok(())
    }

    /// A detached diagnostic copy. Editing it never changes installed state.
    pub fn module_instance_snapshot(&self, module: &LoadedModule) -> Option<ModuleInstance> {
        self.validate_module_state_access(module).ok()?;
        self.modules.instance_snapshot(module.key())
    }

    /// Read a checked execution slot. Drivers must root the result across safepoints.
    pub fn read_module_slot(
        &self,
        module: &LoadedModule,
        slot: ModuleSlot,
    ) -> Result<Value, RuntimeError> {
        self.validate_module_state_access(module)?;
        let declaration = slot_declaration(module, slot)?;
        let value = self.modules.read_slot(module, slot).ok_or_else(|| {
            self.resources()
                .quarantine("loaded module slot disappeared")
        })?;
        if !value.has_representation(declaration.ty)
            || !value.is_storable()
            || !self.gc.validate_value(&value)
        {
            return Err(self.resources().quarantine("invalid value in module slot"));
        }
        Ok(value)
    }

    /// Validate before replacing an edge in installed module storage.
    /// No collection or user callback runs between validation and publication.
    pub fn write_module_slot(
        &self,
        module: &LoadedModule,
        slot: ModuleSlot,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.validate_module_state_access(module)?;
        let declaration = slot_declaration(module, slot)?;
        if !declaration.mutable {
            return Err(RuntimeError::module_validation("immutable module slot"));
        }
        if !value.has_representation(declaration.ty) {
            return Err(RuntimeError::module_validation(
                "module slot representation mismatch",
            ));
        }
        self.validate_heap_payloads(slice::from_ref(&value))?;
        if self.modules.is_staged(module)
            && !self
                .gc
                .validate_candidate_value_for(module.program_root().key(), &value)
        {
            return Err(RuntimeError::execution_phase_violation(
                "external object in candidate module state",
            ));
        }
        // Detach the old edge only after all checks; dispose outside the store borrow.
        let previous = self
            .modules
            .replace_slot(module, slot, value)
            .ok_or_else(|| {
                self.resources()
                    .quarantine("loaded module slot disappeared")
            })?;
        drop(previous);
        Ok(())
    }
}

fn slot_declaration(
    module: &LoadedModule,
    slot: ModuleSlot,
) -> Result<&BytecodeModuleSlot, RuntimeError> {
    module
        .bytecode
        .module_slots
        .get(slot.index())
        .ok_or_else(|| RuntimeError::module_validation("module slot index out of bounds"))
}

impl ModuleStore {
    fn read_slot(&self, module: &LoadedModule, slot: ModuleSlot) -> Option<Value> {
        let records = self.inner.try_borrow().ok()?;
        records
            .resolve(module)?
            .instance
            .module_slots
            .get(slot.index())
            .cloned()
    }

    fn replace_slot(&self, module: &LoadedModule, slot: ModuleSlot, value: Value) -> Option<Value> {
        let mut records = self.inner.try_borrow_mut().ok()?;
        let slots = &mut records.resolve_mut(module)?.instance.module_slots;
        Some(mem::replace(slots.get_mut(slot.index())?, value))
    }
}

#[cfg(test)]
mod tests;
