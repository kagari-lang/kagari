use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    execution_state::ExecutionState,
    frame::values::ExecutionValues,
    publication::PublicationState,
    session::{
        SessionState,
        store::{SessionId, SessionStore},
    },
};
#[cfg(test)]
use std::sync::{Arc, Weak};
use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    mem,
    num::NonZeroUsize,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsyncLimits {
    pub max_pending_operations: NonZeroUsize,
    pub max_tasks: NonZeroUsize,
    pub max_task_scopes: NonZeroUsize,
    pub max_task_waiters: NonZeroUsize,
    pub max_scheduled_invocations: NonZeroUsize,
    pub max_task_failure_events: NonZeroUsize,
    pub max_groups: NonZeroUsize,
    pub max_group_depth: NonZeroUsize,
}

impl Default for AsyncLimits {
    fn default() -> Self {
        Self {
            max_pending_operations: NonZeroUsize::new(1024).expect("positive default"),
            max_tasks: NonZeroUsize::new(1024).expect("positive default"),
            max_task_scopes: NonZeroUsize::new(64).expect("positive default"),
            max_task_waiters: NonZeroUsize::new(4096).expect("positive default"),
            max_scheduled_invocations: NonZeroUsize::new(1024).expect("positive default"),
            max_task_failure_events: NonZeroUsize::new(256).expect("positive default"),
            max_groups: NonZeroUsize::new(1024).expect("positive default"),
            max_group_depth: NonZeroUsize::new(64).expect("positive default"),
        }
    }
}

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

/// Committed payload occupancy and attributed backing pressure. Bytes exclude
/// referenced GC objects and host-owned resources; providers document estimates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeapCharge {
    pub logical_units: usize,
    pub retained_bytes: usize,
}

impl HeapCharge {
    pub fn new(logical_units: usize, retained_bytes: usize) -> Self {
        Self {
            logical_units,
            retained_bytes,
        }
    }

    pub(crate) fn with_shallow_bytes(self, bytes: usize) -> Result<Self, RuntimeError> {
        Ok(Self {
            logical_units: self.logical_units,
            retained_bytes: self
                .retained_bytes
                .checked_add(bytes)
                .ok_or_else(|| RuntimeError::resource_limit("native retained bytes"))?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceCounters {
    pub current_call_depth: u32,
    pub peak_call_depth: u32,
    pub current_heap_units: usize,
    pub peak_heap_units: usize,
    /// Attributed backing bytes, not RSS or total runtime memory. The wide sum
    /// permits preflighting any usize-sized storage change before reserving it.
    pub current_retained_bytes: u128,
    pub peak_retained_bytes: u128,
}

/// Monotonic committed growth, independent of current occupancy and releases.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommittedPressure {
    pub(crate) units: u128,
    pub(crate) bytes: u128,
}

#[derive(Debug)]
pub struct ResourceState {
    // Observe destruction in ownership regressions without making storage shared.
    #[cfg(test)]
    lifetime: Arc<()>,
    active_session: Cell<Option<SessionId>>,
    pub(crate) publication: PublicationState,
    pub(crate) sessions: SessionStore,
    pub(crate) frame_values: RefCell<ExecutionValues>,
    execution: ExecutionState,
    limits: RuntimeLimits,
    counters: RefCell<ResourceCounters>,
    pressure: Cell<CommittedPressure>,
}

/// Exclusive, preflighted transition; no callbacks or reentry while held.
pub(crate) struct HeapChange<'a> {
    session: Option<Ref<'a, SessionState>>,
    counters: RefMut<'a, ResourceCounters>,
    pressure: &'a Cell<CommittedPressure>,
    before: HeapCharge,
    base_units: usize,
    base_bytes: u128,
    max_units: usize,
}

impl HeapChange<'_> {
    pub(crate) fn commit(mut self, charge: HeapCharge) {
        assert!(
            charge.logical_units <= self.max_units,
            "preflighted heap change"
        );
        let mut pressure = self.pressure.get();
        pressure.units += charge
            .logical_units
            .saturating_sub(self.before.logical_units) as u128;
        pressure.bytes += charge
            .retained_bytes
            .saturating_sub(self.before.retained_bytes) as u128;
        self.pressure.set(pressure);
        let units = self.base_units + charge.logical_units;
        let bytes = self.base_bytes + charge.retained_bytes as u128;
        self.counters.current_heap_units = units;
        self.counters.peak_heap_units = self.counters.peak_heap_units.max(units);
        self.counters.current_retained_bytes = bytes;
        self.counters.peak_retained_bytes = self.counters.peak_retained_bytes.max(bytes);
        if let Some(session) = self.session {
            session
                .peak_heap_units
                .set(session.peak_heap_units.get().max(units));
        }
    }
}

/// Account temporary logical preparation units, not a backing-byte estimate.
pub(crate) struct TemporaryHeap<'a> {
    resources: &'a ResourceState,
    units: usize,
}

impl Drop for TemporaryHeap<'_> {
    fn drop(&mut self) {
        self.resources.release_heap_units(self.units);
    }
}

impl ResourceState {
    #[cfg(test)]
    pub(crate) fn lifetime_probe(&self) -> Weak<()> {
        Arc::downgrade(&self.lifetime)
    }

    pub(crate) fn collect_operation<T>(
        &self,
        collect: impl FnOnce() -> T,
    ) -> Result<T, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.execution.collect(collect)
    }

    pub fn new(limits: RuntimeLimits) -> Self {
        Self {
            #[cfg(test)]
            lifetime: Arc::new(()),
            active_session: Cell::new(None),
            publication: PublicationState::default(),
            sessions: SessionStore::default(),
            frame_values: RefCell::default(),
            execution: Default::default(),
            limits,
            counters: RefCell::new(ResourceCounters::default()),
            pressure: Cell::default(),
        }
    }

    pub(crate) fn active_session(&self) -> Option<Ref<'_, SessionState>> {
        self.active_session
            .get()
            .and_then(|id| self.sessions.get(id))
    }

    pub(crate) fn replace_session(&self, session: Option<SessionId>) -> Option<SessionId> {
        self.active_session.replace(session)
    }

    pub(crate) fn start_execution(&self, session: SessionId) {
        self.active_session.set(Some(session));
    }

    pub(crate) fn end_execution(&self, session: SessionId) {
        if self.active_session.get() == Some(session) {
            self.active_session.set(None);
        }
    }

    pub fn termination(&self) -> Option<RuntimeError> {
        self.active_session()
            .as_ref()
            .and_then(|session| session.termination.borrow().clone())
    }

    pub fn poll_execution(&self) -> Result<(), RuntimeError> {
        self.execution.ensure_allowed()?;
        // Session polling checks sticky termination before cancellation. Resolve
        // that session once; no callback can change it during this operation.
        if let Some(session) = self.active_session().as_ref() {
            session.poll(self)?;
        }
        Ok(())
    }

    /// Admit a failed execution operation before unwinding its frames.
    ///
    /// Resource exhaustion and cancellation terminate the active session, preserving
    /// its first termination and origin. Ordinary traps pass through unchanged.
    /// Without an active session this only returns the supplied error; detached
    /// errors and business-value admission rejections must not call this service.
    pub fn admit_execution_error(&self, error: RuntimeError) -> RuntimeError {
        if matches!(
            error.kind(),
            RuntimeErrorKind::ResourceLimitExceeded | RuntimeErrorKind::Cancelled
        ) && let Some(session) = self.active_session()
        {
            return session.terminate(self, error);
        }
        error
    }

    pub(crate) fn limit(&self, name: &'static str) -> RuntimeError {
        self.admit_execution_error(RuntimeError::resource_limit(name))
    }

    pub(crate) fn close_execution(&self) {
        self.execution.close();
    }

    #[inline]
    pub fn ensure_execution_allowed(&self) -> Result<(), RuntimeError> {
        self.admit_active_session().map(drop)
    }

    /// Check permission and lend the exact active owner for immediate internal
    /// scope validation. No callback or execution transition may retain this borrow.
    #[inline]
    pub(crate) fn admit_active_session(
        &self,
    ) -> Result<Option<Ref<'_, SessionState>>, RuntimeError> {
        self.execution.ensure_allowed()?;
        let session = self.active_session();
        if let Some(session) = &session {
            session.check_termination()?;
        }
        Ok(session)
    }

    /// Inspect retained data even after execution closes or a session cancels.
    /// Collector/commit exclusion remains authoritative and cannot be bypassed.
    pub(crate) fn ensure_inspection_allowed(&self) -> Result<(), RuntimeError> {
        self.execution.ensure_inspection_allowed()
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

    pub(crate) fn committed_pressure(&self) -> CommittedPressure {
        self.pressure.get()
    }

    pub fn counters(&self) -> ResourceCounters {
        *self.counters.borrow()
    }

    pub(crate) fn park_call_depth(&self) -> u32 {
        mem::take(&mut self.counters.borrow_mut().current_call_depth)
    }

    pub(crate) fn restore_call_depth(&self, depth: u32) {
        self.counters.borrow_mut().current_call_depth = depth;
    }

    pub(crate) fn enter_call(&self) -> Result<(), RuntimeError> {
        self.poll_execution()?;
        let mut counters = self.counters.borrow_mut();
        let next = counters
            .current_call_depth
            .checked_add(1)
            .ok_or_else(|| self.limit("call depth"))?;
        let base = self
            .active_session()
            .map_or(0, |session| session.depth_base.get());
        let logical_depth = next
            .checked_add(base)
            .ok_or_else(|| self.limit("call depth"))?;
        if let Some(max) = self.limits.max_call_depth
            && logical_depth > max
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

    pub(crate) fn reserve_temporary_heap(
        &self,
        units: usize,
    ) -> Result<TemporaryHeap<'_>, RuntimeError> {
        self.prepare_heap_change(HeapCharge::default(), units)?
            .commit(HeapCharge::new(units, 0));
        Ok(TemporaryHeap {
            resources: self,
            units,
        })
    }

    pub(crate) fn prepare_heap_change(
        &self,
        before: HeapCharge,
        max_units: usize,
    ) -> Result<HeapChange<'_>, RuntimeError> {
        self.poll_execution()?;
        let mut change = self.prepare_cleanup_heap_change(before, max_units)?;
        change.session = self.active_session();
        Ok(change)
    }

    /// Only sealed cleanup may bypass execution admission. It cannot invoke user
    /// code; already terminated sessions still need to publish Task failures.
    pub(crate) fn prepare_cleanup_heap_change(
        &self,
        before: HeapCharge,
        max_units: usize,
    ) -> Result<HeapChange<'_>, RuntimeError> {
        let counters = self.counters.borrow_mut();
        let base_units = counters
            .current_heap_units
            .checked_sub(before.logical_units)
            .ok_or_else(|| self.quarantine("heap charge exceeds committed occupancy"))?;
        let base_bytes = counters
            .current_retained_bytes
            .checked_sub(before.retained_bytes as u128)
            .ok_or_else(|| self.quarantine("heap charge exceeds committed backing"))?;
        base_units
            .checked_add(max_units)
            .ok_or_else(|| self.limit("heap units"))?;
        // Actual Vec capacity is known only after reserve. Preflight the entire
        // representable per-object range, not its requested minimum capacity.
        base_bytes
            .checked_add(usize::MAX as u128)
            .ok_or_else(|| self.limit("retained bytes"))?;
        let pressure = self.pressure.get();
        pressure
            .units
            .checked_add(max_units as u128)
            .ok_or_else(|| self.limit("committed heap pressure"))?;
        pressure
            .bytes
            .checked_add(usize::MAX as u128)
            .ok_or_else(|| self.limit("committed backing pressure"))?;
        Ok(HeapChange {
            session: None,
            counters,
            pressure: &self.pressure,
            before,
            base_units,
            base_bytes,
            max_units,
        })
    }

    pub(crate) fn release_heap_charge(&self, charge: HeapCharge) {
        self.release_heap_totals(charge.logical_units, charge.retained_bytes as u128);
    }

    /// Release one already validated synchronous retirement batch atomically.
    pub(crate) fn release_heap_totals(&self, logical_units: usize, retained_bytes: u128) {
        let mut counters = self.counters.borrow_mut();
        let Some(units) = counters.current_heap_units.checked_sub(logical_units) else {
            self.quarantine("heap charge underflow during cleanup");
            return;
        };
        let Some(bytes) = counters.current_retained_bytes.checked_sub(retained_bytes) else {
            self.quarantine("backing charge underflow during cleanup");
            return;
        };
        counters.current_heap_units = units;
        counters.current_retained_bytes = bytes;
    }

    pub(crate) fn release_heap_units(&self, units: usize) {
        self.release_heap_charge(HeapCharge::new(units, 0));
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
    fn positive_pressure_survives_release_and_failed_preparation() {
        let resources = ResourceState::default();
        resources
            .prepare_heap_change(HeapCharge::default(), 3)
            .unwrap()
            .commit(HeapCharge::new(3, 32));
        drop(
            resources
                .prepare_heap_change(HeapCharge::new(3, 32), 9)
                .unwrap(),
        );
        assert_eq!(
            resources.committed_pressure(),
            CommittedPressure {
                units: 3,
                bytes: 32
            }
        );
        resources
            .prepare_heap_change(HeapCharge::new(3, 32), 3)
            .unwrap()
            .commit(HeapCharge::new(1, 64));
        resources.release_heap_charge(HeapCharge::new(1, 64));
        assert_eq!(
            resources.committed_pressure(),
            CommittedPressure {
                units: 3,
                bytes: 64
            }
        );
        assert_eq!(resources.counters().current_heap_units, 0);
        resources
            .prepare_heap_change(HeapCharge::default(), 2)
            .unwrap()
            .commit(HeapCharge::new(2, 8));
        assert_eq!(
            resources.committed_pressure(),
            CommittedPressure {
                units: 5,
                bytes: 72
            }
        );
    }

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

#[cfg(test)]
mod mutation_resources;
