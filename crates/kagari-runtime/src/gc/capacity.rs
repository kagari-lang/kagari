use crate::gc::hash_storage::{HashMapStorage, HashSetStorage};
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::GcHeap,
    value::Value,
};

fn invalid() -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid capacity receiver")
}
impl GcHeap {
    pub fn collection_capacity(&self, value: &Value) -> Result<usize, RuntimeError> {
        self.ensure_execution_allowed()?;
        match value {
            Value::Array(id) => self.with_array(*id, Vec::capacity),
            Value::Map(id) => self.with_map(*id, HashMapStorage::capacity),
            Value::Set(id) => self.with_set(*id, HashSetStorage::capacity),
            _ => None,
        }
        .ok_or_else(invalid)
    }
    pub fn reserve_collection(&self, value: &Value, additional: usize) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        match value {
            Value::Array(id) | Value::Map(id) | Value::Set(id) => {
                self.ensure_callback_mutable(*id)?
            }
            _ => return Err(invalid()),
        }
        let (length, capacity, units) = match value {
            Value::Array(id) => self.with_array(*id, |v| (v.len(), v.capacity(), 1)),
            Value::Map(id) => self.with_map(*id, |v| (v.len(), v.capacity(), 2)),
            Value::Set(id) => self.with_set(*id, |v| (v.len(), v.capacity(), 1)),
            _ => None,
        }
        .ok_or_else(invalid)?;
        let desired = length
            .checked_add(additional)
            .ok_or_else(|| self.resource_limit("collection capacity"))?;
        if desired <= capacity {
            return Ok(());
        }
        let requested = desired
            .checked_mul(units)
            .ok_or_else(|| self.resource_limit("collection capacity"))?;
        // Capacity is not a live Value slot. Charge preparation/allocation work,
        // while the existing live heap counter continues to count stored values.
        let _temporary = self.resources.reserve_temporary_heap(requested)?;
        let allocation = || self.resource_limit("collection capacity");
        match value {
            Value::Array(id) => {
                self.with_array_mut(*id, |v| v.try_reserve(additional).map_err(|_| allocation()))
            }
            Value::Map(id) => {
                self.with_map_mut(*id, |v| v.try_reserve(additional).map_err(|_| allocation()))
            }
            Value::Set(id) => {
                self.with_set_mut(*id, |v| v.try_reserve(additional).map_err(|_| allocation()))
            }
            _ => None,
        }
        .ok_or_else(invalid)?
    }
}
