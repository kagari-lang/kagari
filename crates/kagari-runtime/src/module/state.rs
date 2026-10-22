//! Ready-only access to installation-owned persistent cells.
pub(super) mod cells;
mod compatibility;
pub(crate) mod initialization;

use crate::{
    Runtime,
    error::RuntimeError,
    module::{LoadedModule, ModuleInstance, ModuleStore},
    value::Value,
};
use kagari_bytecode::instruction::ModuleSlot;
use kagari_common::span::Span;
use kagari_contract::ids::FunctionRef;
use std::{fmt, slice};

/// A fixed introducing-generation recipe. A copy grants no initialization authority.
#[derive(Debug, Clone)]
pub struct StaticRecipe {
    pub module: LoadedModule,
    pub function: FunctionRef,
}

/// Detached diagnostic state. Only runtime-owned transitions can change a cell.
#[derive(Debug, Clone)]
pub enum StaticState {
    Uninitialized(StaticRecipe),
    Initializing(StaticRecipe),
    Ready(Value),
    /// Bounded, owned diagnostic text; no frame, execution session or borrow is retained.
    Failed(StaticFailure),
}

/// Bounded diagnostic data, independent of frames, sessions and executable ownership.
#[derive(Debug, Clone)]
pub struct StaticFailure {
    cause: String,
    origin: Option<Span>,
}

impl StaticFailure {
    /// Retain at most 1024 UTF-8 bytes of the cause and an optional initializer span.
    /// Constructing a diagnostic does not grant access to a cell or its transitions.
    pub fn new(cause: &str, origin: Option<Span>) -> Self {
        let mut end = cause.len().min(1024);
        while !cause.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            cause: cause[..end].to_owned(),
            origin,
        }
    }

    pub fn cause(&self) -> &str {
        &self.cause
    }

    pub fn origin(&self) -> Option<Span> {
        self.origin
    }
}

impl fmt::Display for StaticFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.cause)
    }
}

impl Runtime {
    pub(in crate::module) fn validate_module_state_access(
        &self,
        module: &LoadedModule,
    ) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        self.validate_loaded_module(module)?;
        if self.is_candidate_initialization() || self.modules.is_staged(module) {
            return Err(RuntimeError::execution_phase_violation(
                "candidate cannot access static state",
            ));
        }
        Ok(())
    }

    /// Inspect without forcing initialization. Editing the copy cannot change state.
    pub fn module_instance_snapshot(&self, module: &LoadedModule) -> Option<ModuleInstance> {
        self.validate_module_state_access(module).ok()?;
        self.modules.instance_snapshot(module.key())
    }

    /// Read an already initialized cell; never executes its recipe.
    /// Drivers must root the result across safepoints.
    pub fn read_module_slot(
        &self,
        module: &LoadedModule,
        slot: ModuleSlot,
    ) -> Result<Value, RuntimeError> {
        self.validate_module_state_access(module)?;
        let contract = module
            .bytecode
            .module_slots
            .get(slot.index())
            .ok_or_else(|| RuntimeError::module_validation("static binding index out of bounds"))?;
        let state = self.modules.static_state(module, slot).ok_or_else(|| {
            self.resources()
                .quarantine("loaded static binding disappeared")
        })?;
        match state {
            StaticState::Ready(value) => {
                self.validate_heap_payloads(slice::from_ref(&value))?;
                if !self.matches_type_in(&value, &contract.contract.ty, module, None) {
                    return Err(self
                        .resources()
                        .quarantine("invalid semantic value in static cell"));
                }
                Ok(value)
            }
            StaticState::Uninitialized(_) => Err(RuntimeError::module_validation(
                "static initialization requires an execution transition",
            )),
            StaticState::Initializing(_) => {
                self.poison_static_cycle(module, slot)?;
                Err(RuntimeError::module_validation(
                    "recursive static initialization",
                ))
            }
            StaticState::Failed(cause) => Err(RuntimeError::module_validation(format!(
                "static initialization failed: {cause}"
            ))),
        }
    }

    /// Replace a ready mutable cell after complete semantic and owner admission.
    /// This is not an initialization API, including for mutable declarations.
    pub fn write_module_slot(
        &self,
        module: &LoadedModule,
        slot: ModuleSlot,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.validate_module_state_access(module)?;
        let descriptor = module
            .bytecode
            .module_slots
            .get(slot.index())
            .ok_or_else(|| RuntimeError::module_validation("static binding index out of bounds"))?;
        if !descriptor.contract.mutable {
            return Err(RuntimeError::module_validation("immutable static binding"));
        }
        self.read_module_slot(module, slot)?;
        self.validate_heap_payloads(slice::from_ref(&value))?;
        if !self.matches_type_in(&value, &descriptor.contract.ty, module, None) {
            return Err(RuntimeError::module_validation(
                "static semantic type mismatch",
            ));
        }
        self.modules
            .replace_static(module, slot, value)
            .ok_or_else(|| {
                self.resources()
                    .quarantine("loaded static binding disappeared")
            })?;
        Ok(())
    }
}

impl ModuleStore {
    fn static_state(&self, module: &LoadedModule, slot: ModuleSlot) -> Option<StaticState> {
        let store = self.inner.try_borrow().ok()?;
        let record = store.resolve(module)?;
        Some(store.bound_cell(record, slot.index())?.snapshot())
    }

    fn replace_static(&self, module: &LoadedModule, slot: ModuleSlot, value: Value) -> Option<()> {
        let mut store = self.inner.try_borrow_mut().ok()?;
        let record = store.resolve(module)?;
        store.bound_cell(record, slot.index())?;
        let id = *record.bindings.get(slot.index())?;
        let cell = store.cells.get_mut(&id)?;
        if !matches!(cell.state, StaticState::Ready(_)) {
            return None;
        }
        cell.state = StaticState::Ready(value);
        Some(())
    }
}

#[cfg(test)]
pub(crate) mod tests;
