use crate::{error::RuntimeError, execution_state::ExecutionState, session::SessionState};
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

/// Runtime-wide protection against accidental recursion. No execution metering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLimits {
    pub max_call_depth: Option<u32>,
}
impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            max_call_depth: Some(256),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceCounters {
    pub current_call_depth: u32,
    pub peak_call_depth: u32,
    pub current_heap_units: usize,
    pub peak_heap_units: usize,
    pub loaded_modules: usize,
}

#[derive(Debug)]
pub struct ResourceState {
    active_session: RefCell<Option<Rc<SessionState>>>,
    execution: ExecutionState,
    limits: RuntimeLimits,
    counters: RefCell<ResourceCounters>,
}

/// A checked, uncharged growth operation. No user code runs while it is held.
pub(crate) struct HeapGrowth<'a> {
    session: Option<Rc<SessionState>>,
    counters: RefMut<'a, ResourceCounters>,
    live: usize,
}

/// Account temporary native storage until preparation commits or fails.
pub(crate) struct TemporaryHeap<'a> {
    resources: &'a ResourceState,
    units: usize,
}
impl Drop for TemporaryHeap<'_> {
    fn drop(&mut self) {
        self.resources.release_heap_units(self.units);
    }
}
impl HeapGrowth<'_> {
    pub(crate) fn commit(mut self) {
        self.counters.current_heap_units = self.live;
        self.counters.peak_heap_units = self.counters.peak_heap_units.max(self.live);
        if let Some(session) = self.session {
            session
                .peak_heap_units
                .set(session.peak_heap_units.get().max(self.live));
        }
    }
}

impl ResourceState {
    pub fn new(limits: RuntimeLimits) -> Self {
        Self {
            active_session: RefCell::new(None),
            execution: Default::default(),
            limits,
            counters: RefCell::new(ResourceCounters::default()),
        }
    }

    pub(crate) fn active_session(&self) -> Option<Rc<SessionState>> {
        self.active_session.borrow().clone()
    }

    pub(crate) fn replace_session(
        &self,
        session: Option<Rc<SessionState>>,
    ) -> Option<Rc<SessionState>> {
        self.active_session.replace(session)
    }

    pub(crate) fn start_execution(&self, session: Rc<SessionState>) {
        *self.active_session.borrow_mut() = Some(session);
    }

    pub(crate) fn end_execution(&self, session: &Rc<SessionState>) {
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
        let _next = current
            .checked_add(1)
            .ok_or_else(|| self.limit("dirty records"))?;
        Ok(())
    }

    pub fn counters(&self) -> ResourceCounters {
        *self.counters.borrow()
    }

    pub(crate) fn enter_call(&self) -> Result<(), RuntimeError> {
        self.poll_execution()?;
        let mut counters = self.counters.borrow_mut();
        let next = counters
            .current_call_depth
            .checked_add(1)
            .ok_or_else(|| self.limit("call depth"))?;
        if let Some(max) = self.limits.max_call_depth
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
        Ok(HeapGrowth {
            session: self.active_session(),
            counters,
            live,
        })
    }

    pub(crate) fn reserve_temporary_heap(
        &self,
        units: usize,
    ) -> Result<TemporaryHeap<'_>, RuntimeError> {
        self.prepare_heap_growth(units)?.commit();
        Ok(TemporaryHeap {
            resources: self,
            units,
        })
    }

    pub(crate) fn release_heap_units(&self, units: usize) {
        let mut counters = self.counters.borrow_mut();
        counters.current_heap_units = counters
            .current_heap_units
            .checked_sub(units)
            .expect("heap accounting cannot underflow");
    }

    pub(crate) fn admit_modules(&self, additional: usize) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let mut counters = self.counters.borrow_mut();
        let next = counters
            .loaded_modules
            .checked_add(additional)
            .ok_or_else(|| self.limit("loaded modules"))?;
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
        Self::new(RuntimeLimits::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_call_depth_peaks() {
        let resources = ResourceState::new(RuntimeLimits {
            max_call_depth: Some(2),
        });

        resources.enter_call().unwrap();
        resources.enter_call().unwrap();
        assert_eq!(resources.counters().peak_call_depth, 2);
        assert!(resources.enter_call().is_err());
        resources.leave_call();
        assert_eq!(resources.counters().current_call_depth, 1);
    }
}
