use std::cell::RefCell;

use crate::error::RuntimeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourcePolicy {
    pub max_instruction_steps: Option<u64>,
    pub max_call_depth: Option<u32>,
    pub max_heap_units: Option<usize>,
    pub max_allocation_units: Option<usize>,
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
    pub loaded_modules: usize,
    pub elapsed_wall_time_ms: u64,
}

#[derive(Debug)]
pub struct ResourceState {
    policy: ResourcePolicy,
    counters: RefCell<ResourceCounters>,
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
        let mut counters = self.counters.borrow_mut();
        let next = counters.instruction_steps.saturating_add(1);
        if let Some(max) = self.policy.max_instruction_steps {
            if next > max {
                return Err(RuntimeError::resource_limit("instruction steps"));
            }
        }
        counters.instruction_steps = next;
        Ok(())
    }

    pub fn enter_call(&self) -> Result<(), RuntimeError> {
        let mut counters = self.counters.borrow_mut();
        let next = counters.current_call_depth.saturating_add(1);
        if let Some(max) = self.policy.max_call_depth {
            if next > max {
                return Err(RuntimeError::resource_limit("call depth"));
            }
        }
        counters.current_call_depth = next;
        counters.peak_call_depth = counters.peak_call_depth.max(next);
        Ok(())
    }

    pub fn leave_call(&self) {
        let mut counters = self.counters.borrow_mut();
        counters.current_call_depth = counters.current_call_depth.saturating_sub(1);
    }

    pub fn record_heap_units(&self, current: usize, peak: usize) -> Result<(), RuntimeError> {
        if let Some(max) = self.policy.max_heap_units {
            if current > max {
                return Err(RuntimeError::resource_limit("heap units"));
            }
        }

        let mut counters = self.counters.borrow_mut();
        counters.current_heap_units = current;
        counters.peak_heap_units = counters.peak_heap_units.max(peak);
        Ok(())
    }

    pub fn consume_allocation_units(&self, units: usize) -> Result<(), RuntimeError> {
        let mut counters = self.counters.borrow_mut();
        let next = counters.allocation_units.saturating_add(units);
        if let Some(max) = self.policy.max_allocation_units {
            if next > max {
                return Err(RuntimeError::resource_limit("allocation units"));
            }
        }
        counters.allocation_units = next;
        Ok(())
    }

    pub fn record_loaded_modules(&self, loaded_modules: usize) -> Result<(), RuntimeError> {
        if let Some(max) = self.policy.max_modules {
            if loaded_modules > max {
                return Err(RuntimeError::resource_limit("loaded modules"));
            }
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
}
