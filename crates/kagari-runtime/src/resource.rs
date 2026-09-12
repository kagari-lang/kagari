use std::cell::{RefCell, RefMut};

use crate::error::RuntimeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourcePolicy {
    pub max_instruction_steps: Option<u64>,
    pub max_call_depth: Option<u32>,
    pub max_heap_units: Option<usize>,
    pub max_allocation_units: Option<usize>,
    pub max_host_calls: Option<u64>,
    pub max_reflection_operations: Option<u64>,
    pub max_modules: Option<usize>,
    pub max_wall_time_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceCounters {
    pub instruction_steps: u64,
    pub current_call_depth: u32,
    pub peak_call_depth: u32,
    pub current_heap_units: usize,
    pub peak_heap_units: usize,
    pub allocation_units: usize,
    pub host_calls: u64,
    pub reflection_operations: u64,
    pub loaded_modules: usize,
    pub elapsed_wall_time_ms: u64,
}

#[derive(Debug)]
pub struct ResourceState {
    policy: ResourcePolicy,
    counters: RefCell<ResourceCounters>,
}

/// A checked, uncharged growth operation. No user code runs while it is held.
pub(crate) struct HeapGrowth<'a> {
    counters: RefMut<'a, ResourceCounters>,
    live: usize,
    allocated: usize,
}
impl HeapGrowth<'_> {
    pub(crate) fn commit(mut self) {
        self.counters.current_heap_units = self.live;
        self.counters.peak_heap_units = self.counters.peak_heap_units.max(self.live);
        self.counters.allocation_units = self.allocated;
    }
}

impl ResourceState {
    pub fn new(policy: ResourcePolicy) -> Self {
        Self {
            policy,
            counters: RefCell::new(ResourceCounters::default()),
        }
    }

    pub fn policy(&self) -> ResourcePolicy {
        self.policy
    }

    pub fn counters(&self) -> ResourceCounters {
        *self.counters.borrow()
    }

    pub fn consume_instruction_step(&self) -> Result<(), RuntimeError> {
        self.consume_instruction_steps(1)
    }

    pub fn consume_instruction_steps(&self, steps: u64) -> Result<(), RuntimeError> {
        let mut counters = self.counters.borrow_mut();
        let next = counters.instruction_steps.saturating_add(steps);
        if let Some(max) = self.policy.max_instruction_steps
            && next > max
        {
            return Err(RuntimeError::resource_limit("instruction steps"));
        }
        counters.instruction_steps = next;
        Ok(())
    }

    pub fn enter_call(&self) -> Result<(), RuntimeError> {
        let mut counters = self.counters.borrow_mut();
        let next = counters.current_call_depth.saturating_add(1);
        if let Some(max) = self.policy.max_call_depth
            && next > max
        {
            return Err(RuntimeError::resource_limit("call depth"));
        }
        counters.current_call_depth = next;
        counters.peak_call_depth = counters.peak_call_depth.max(next);
        Ok(())
    }

    pub fn leave_call(&self) {
        let mut counters = self.counters.borrow_mut();
        counters.current_call_depth = counters.current_call_depth.saturating_sub(1);
    }

    pub(crate) fn prepare_heap_growth(&self, units: usize) -> Result<HeapGrowth<'_>, RuntimeError> {
        let counters = self.counters.borrow_mut();
        let live = counters
            .current_heap_units
            .checked_add(units)
            .ok_or_else(|| RuntimeError::resource_limit("heap units"))?;
        let allocated = counters
            .allocation_units
            .checked_add(units)
            .ok_or_else(|| RuntimeError::resource_limit("allocation units"))?;
        if self.policy.max_heap_units.is_some_and(|max| live > max) {
            return Err(RuntimeError::resource_limit("heap units"));
        }
        if self
            .policy
            .max_allocation_units
            .is_some_and(|max| allocated > max)
        {
            return Err(RuntimeError::resource_limit("allocation units"));
        }
        Ok(HeapGrowth {
            counters,
            live,
            allocated,
        })
    }

    pub(crate) fn release_heap_units(&self, units: usize) {
        let mut counters = self.counters.borrow_mut();
        counters.current_heap_units = counters
            .current_heap_units
            .checked_sub(units)
            .expect("heap accounting cannot underflow");
    }

    pub fn consume_host_call(&self) -> Result<(), RuntimeError> {
        let mut counters = self.counters.borrow_mut();
        let next = counters.host_calls.saturating_add(1);
        if let Some(max) = self.policy.max_host_calls
            && next > max
        {
            return Err(RuntimeError::resource_limit("host calls"));
        }
        counters.host_calls = next;
        Ok(())
    }

    pub fn consume_reflection_operation(&self) -> Result<(), RuntimeError> {
        let mut counters = self.counters.borrow_mut();
        let next = counters.reflection_operations.saturating_add(1);
        if let Some(max) = self.policy.max_reflection_operations
            && next > max
        {
            return Err(RuntimeError::resource_limit("reflection operations"));
        }
        counters.reflection_operations = next;
        Ok(())
    }

    pub fn record_loaded_modules(&self, loaded_modules: usize) -> Result<(), RuntimeError> {
        if let Some(max) = self.policy.max_modules
            && loaded_modules > max
        {
            return Err(RuntimeError::resource_limit("loaded modules"));
        }
        self.counters.borrow_mut().loaded_modules = loaded_modules;
        Ok(())
    }
}

impl Default for ResourceState {
    fn default() -> Self {
        Self::new(ResourcePolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RuntimeErrorKind;

    #[test]
    fn enforces_instruction_step_limits() {
        let resources = ResourceState::new(ResourcePolicy {
            max_instruction_steps: Some(1),
            ..ResourcePolicy::default()
        });

        assert!(resources.consume_instruction_step().is_ok());
        let error = resources.consume_instruction_step().unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        assert_eq!(resources.counters().instruction_steps, 1);
    }

    #[test]
    fn enforces_bulk_instruction_step_limits() {
        let resources = ResourceState::new(ResourcePolicy {
            max_instruction_steps: Some(3),
            ..ResourcePolicy::default()
        });

        resources.consume_instruction_steps(2).unwrap();
        let error = resources.consume_instruction_steps(2).unwrap_err();

        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        assert_eq!(resources.counters().instruction_steps, 2);
    }

    #[test]
    fn tracks_call_depth_peaks() {
        let resources = ResourceState::new(ResourcePolicy {
            max_call_depth: Some(2),
            ..ResourcePolicy::default()
        });

        resources.enter_call().unwrap();
        resources.enter_call().unwrap();
        assert_eq!(resources.counters().peak_call_depth, 2);
        assert!(resources.enter_call().is_err());
        resources.leave_call();
        assert_eq!(resources.counters().current_call_depth, 1);
    }

    #[test]
    fn enforces_allocation_host_and_reflection_limits() {
        let resources = ResourceState::new(ResourcePolicy {
            max_allocation_units: Some(2),
            max_host_calls: Some(1),
            max_reflection_operations: Some(1),
            ..ResourcePolicy::default()
        });

        resources.prepare_heap_growth(2).unwrap().commit();
        assert_eq!(
            resources.prepare_heap_growth(1).err().unwrap().kind(),
            RuntimeErrorKind::ResourceLimitExceeded
        );
        assert_eq!(resources.counters().allocation_units, 2);

        resources.consume_host_call().unwrap();
        assert_eq!(
            resources.consume_host_call().unwrap_err().kind(),
            RuntimeErrorKind::ResourceLimitExceeded
        );
        assert_eq!(resources.counters().host_calls, 1);

        resources.consume_reflection_operation().unwrap();
        assert_eq!(
            resources.consume_reflection_operation().unwrap_err().kind(),
            RuntimeErrorKind::ResourceLimitExceeded
        );
        assert_eq!(resources.counters().reflection_operations, 1);
    }
}
