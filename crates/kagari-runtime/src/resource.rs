use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

use crate::error::RuntimeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourcePolicy {
    pub max_dirty_records: Option<usize>,
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
}

#[derive(Debug)]
pub struct ResourceState {
    active_session: RefCell<Option<Rc<crate::session::SessionState>>>,
    execution: crate::execution_state::ExecutionState,
    policy: ResourcePolicy,
    counters: RefCell<ResourceCounters>,
}

/// A checked, uncharged growth operation. No user code runs while it is held.
pub(crate) struct HeapGrowth<'a> {
    session: Option<Rc<crate::session::SessionState>>,
    counters: RefMut<'a, ResourceCounters>,
    live: usize,
    allocated: usize,
}
impl HeapGrowth<'_> {
    pub(crate) fn commit(mut self) {
        self.counters.current_heap_units = self.live;
        self.counters.peak_heap_units = self.counters.peak_heap_units.max(self.live);
        self.counters.allocation_units = self.allocated;
        if let Some(session) = self.session {
            session
                .peak_heap_units
                .set(session.peak_heap_units.get().max(self.live));
        }
    }
}

impl ResourceState {
    pub fn new(policy: ResourcePolicy) -> Self {
        Self {
            active_session: RefCell::new(None),
            execution: Default::default(),
            policy,
            counters: RefCell::new(ResourceCounters::default()),
        }
    }

    pub fn policy(&self) -> ResourcePolicy {
        self.active_session
            .borrow()
            .as_ref()
            .map_or(self.policy, |session| session.options.resources)
    }

    pub(crate) fn active_session(&self) -> Option<Rc<crate::session::SessionState>> {
        self.active_session.borrow().clone()
    }

    pub(crate) fn start_execution(&self, session: Rc<crate::session::SessionState>) {
        *self.active_session.borrow_mut() = Some(session);
    }

    pub(crate) fn end_execution(&self, session: &Rc<crate::session::SessionState>) {
        let mut active = self.active_session.borrow_mut();
        if active
            .as_ref()
            .is_some_and(|active| Rc::ptr_eq(active, session))
        {
            *active = None;
        }
    }

    pub fn termination(&self) -> Option<RuntimeError> {
        self.active_session
            .borrow()
            .as_ref()
            .and_then(|session| session.termination.borrow().clone())
    }

    pub fn poll_execution(&self) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if let Some(session) = self.active_session.borrow().as_ref() {
            session.poll()?;
        }
        Ok(())
    }

    fn baseline(&self) -> ResourceCounters {
        self.active_session
            .borrow()
            .as_ref()
            .map_or(ResourceCounters::default(), |session| session.baseline)
    }

    pub(crate) fn limit(&self, name: &'static str) -> RuntimeError {
        let error = RuntimeError::resource_limit(name);
        self.active_session
            .borrow()
            .as_ref()
            .map_or_else(|| error.clone(), |session| session.terminate(error.clone()))
    }

    pub fn ensure_execution_allowed(&self) -> Result<(), RuntimeError> {
        self.execution.ensure_allowed()?;
        if let Some(error) = self.termination() {
            return Err(error);
        }
        Ok(())
    }

    pub fn is_quarantined(&self) -> bool {
        self.execution.is_quarantined()
    }

    pub(crate) fn quarantine(&self, reason: &'static str) -> RuntimeError {
        self.execution.quarantine(reason)
    }

    pub(crate) fn commit_host_write(&self, commit: impl FnOnce()) -> Result<(), RuntimeError> {
        self.execution.commit(commit)
    }

    pub(crate) fn prepare_dirty_record(&self, current: usize) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let next = current
            .checked_add(1)
            .ok_or_else(|| self.limit("dirty records"))?;
        if self
            .policy()
            .max_dirty_records
            .is_some_and(|limit| next > limit)
        {
            return Err(self.limit("dirty records"));
        }
        Ok(())
    }

    pub fn counters(&self) -> ResourceCounters {
        *self.counters.borrow()
    }

    pub fn consume_instruction_step(&self) -> Result<(), RuntimeError> {
        self.consume_instruction_steps(1)
    }

    pub fn consume_instruction_steps(&self, steps: u64) -> Result<(), RuntimeError> {
        self.poll_execution()?;
        let mut counters = self.counters.borrow_mut();
        let next = counters
            .instruction_steps
            .checked_add(steps)
            .ok_or_else(|| self.limit("instruction steps"))?;
        if let Some(max) = self.policy().max_instruction_steps
            && next - self.baseline().instruction_steps > max
        {
            return Err(self.limit("instruction steps"));
        }
        counters.instruction_steps = next;
        Ok(())
    }

    pub(crate) fn enter_call(&self) -> Result<(), RuntimeError> {
        self.poll_execution()?;
        let mut counters = self.counters.borrow_mut();
        let next = counters
            .current_call_depth
            .checked_add(1)
            .ok_or_else(|| self.limit("call depth"))?;
        if let Some(max) = self.policy().max_call_depth
            && next > max
        {
            return Err(self.limit("call depth"));
        }
        counters.current_call_depth = next;
        counters.peak_call_depth = counters.peak_call_depth.max(next);
        if let Some(session) = self.active_session() {
            session
                .peak_call_depth
                .set(session.peak_call_depth.get().max(next));
        }
        Ok(())
    }

    pub(crate) fn leave_call(&self) {
        let mut counters = self.counters.borrow_mut();
        if let Some(depth) = counters.current_call_depth.checked_sub(1) {
            counters.current_call_depth = depth;
        } else {
            self.quarantine("call depth underflow during frame cleanup");
        }
    }

    pub(crate) fn prepare_heap_growth(&self, units: usize) -> Result<HeapGrowth<'_>, RuntimeError> {
        self.poll_execution()?;
        let counters = self.counters.borrow_mut();
        let live = counters
            .current_heap_units
            .checked_add(units)
            .ok_or_else(|| self.limit("heap units"))?;
        let allocated = counters
            .allocation_units
            .checked_add(units)
            .ok_or_else(|| self.limit("allocation units"))?;
        if self.policy().max_heap_units.is_some_and(|max| live > max) {
            return Err(self.limit("heap units"));
        }
        if self
            .policy()
            .max_allocation_units
            .is_some_and(|max| allocated - self.baseline().allocation_units > max)
        {
            return Err(self.limit("allocation units"));
        }
        Ok(HeapGrowth {
            session: self.active_session(),
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
        self.poll_execution()?;
        let mut counters = self.counters.borrow_mut();
        let next = counters
            .host_calls
            .checked_add(1)
            .ok_or_else(|| self.limit("host calls"))?;
        if let Some(max) = self.policy().max_host_calls
            && next - self.baseline().host_calls > max
        {
            return Err(self.limit("host calls"));
        }
        counters.host_calls = next;
        Ok(())
    }

    pub fn consume_reflection_operation(&self) -> Result<(), RuntimeError> {
        self.poll_execution()?;
        let mut counters = self.counters.borrow_mut();
        let next = counters
            .reflection_operations
            .checked_add(1)
            .ok_or_else(|| self.limit("reflection operations"))?;
        if let Some(max) = self.policy().max_reflection_operations
            && next - self.baseline().reflection_operations > max
        {
            return Err(self.limit("reflection operations"));
        }
        counters.reflection_operations = next;
        Ok(())
    }

    pub(crate) fn admit_modules(&self, additional: usize) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let mut counters = self.counters.borrow_mut();
        let next = counters
            .loaded_modules
            .checked_add(additional)
            .ok_or_else(|| self.limit("loaded modules"))?;
        if let Some(max) = self.policy().max_modules
            && next > max
        {
            return Err(self.limit("loaded modules"));
        }
        counters.loaded_modules = next;
        Ok(())
    }

    /// Releasing ownership must work even after cancellation or quarantine.
    pub(crate) fn release_modules(&self, count: usize) {
        let mut counters = self.counters.borrow_mut();
        match counters.loaded_modules.checked_sub(count) {
            Some(remaining) => counters.loaded_modules = remaining,
            None => {
                self.quarantine("loaded module count underflow");
            }
        }
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
