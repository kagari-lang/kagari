pub(crate) mod candidate;
pub(crate) mod native_stack;
use crate::{
    error::{
        RuntimeError, RuntimeErrorKind,
        fault::EngineFault,
        reason::{CancellationReason, ErrorReason},
    },
    execution::roots::SessionId,
    execution::space::DEFAULT_STACK_BYTES,
    execution_state::ExecutionState,
    frame::uncommon::UncommonFrames,
    host::HostRegistryId,
    module::ModuleKey,
    publication::PublicationState,
    resource::candidate::CandidateOrigin,
    session::{ExecutionPhase, SessionState, store::SessionStore},
};
use kagari_gc::heap::accounting::AccountingReport;

#[cfg(test)]
use std::sync::Weak;
use std::{
    cell::{Cell, Ref, RefCell},
    mem,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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

/// Committed VM storage per execution and independent Native/Rust-stack protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLimits {
    /// Committed frame, slot and execution-source capacity per execution.
    /// Defaults to 8 MiB; excludes managed heap and temporary allocator peaks.
    pub max_stack_bytes: NonZeroUsize,

    /// Nested Native/compiled entries sharing the Runtime's Rust stack.
    /// Defaults to 32. Supported hosts additionally check remaining OS stack.
    pub max_native_depth: NonZeroUsize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            max_stack_bytes: NonZeroUsize::new(DEFAULT_STACK_BYTES).expect("positive default"),
            max_native_depth: NonZeroUsize::new(32).expect("positive default"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceCounters {
    pub current_call_depth: u32,
    pub peak_call_depth: u32,
    /// Managed objects plus provider units and active conversion preparation.
    pub current_heap_units: u128,
    pub peak_heap_units: u128,
    /// Committed provider-owned indirect backing. Excludes heap pages,
    /// descriptor/control storage, external Rust state and process RSS.
    pub current_retained_bytes: u128,
    pub peak_retained_bytes: u128,
}

#[derive(Debug)]
pub struct ResourceState {
    // Observe destruction in ownership regressions without making storage shared.
    #[cfg(test)]
    lifetime: Arc<()>,
    active_session: Cell<Option<SessionId>>,
    static_failures: Arc<AtomicBool>,
    pub(crate) publication: PublicationState,
    pub(crate) sessions: SessionStore,
    pub(crate) uncommon_frames: UncommonFrames,
    execution: ExecutionState,
    limits: RuntimeLimits,
    native_depth: Cell<usize>,
    counters: RefCell<ResourceCounters>,
    temporary_heap_units: Cell<usize>,
}

/// Account temporary logical preparation units, not a backing-byte estimate.
pub(crate) struct TemporaryHeap<'a> {
    resources: &'a ResourceState,
    units: usize,
}

impl Drop for TemporaryHeap<'_> {
    fn drop(&mut self) {
        self.resources.release_temporary_heap(self.units);
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
            static_failures: Arc::new(AtomicBool::new(false)),
            publication: PublicationState::default(),
            sessions: SessionStore::default(),
            uncommon_frames: UncommonFrames::default(),
            execution: Default::default(),
            limits,
            native_depth: Cell::new(0),
            counters: RefCell::new(ResourceCounters::default()),
            temporary_heap_units: Cell::new(0),
        }
    }

    pub(crate) fn static_failure_signal(&self) -> Arc<AtomicBool> {
        self.static_failures.clone()
    }

    pub(crate) fn take_static_failures(&self) -> bool {
        self.static_failures.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn restore_static_failures(&self) {
        self.static_failures.store(true, Ordering::Release);
    }

    pub(crate) fn active_session(&self) -> Option<Ref<'_, SessionState>> {
        self.active_session
            .get()
            .and_then(|id| self.sessions.get(id))
    }

    /// Compare activation identity while the caller already owns a checked
    /// session-record loan. This grants no access to an unborrowed record.
    pub(crate) fn active_session_id(&self) -> Option<SessionId> {
        self.active_session.get()
    }

    pub(crate) fn candidate_owner(&self) -> Option<(HostRegistryId, ModuleKey)> {
        self.active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
            .map(|session| session.root.program_identity())
    }

    /// Capture only immutable birth facts before entering a physical allocator.
    pub(crate) fn candidate_origin(&self) -> CandidateOrigin {
        self.active_session()
            .filter(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
            .map(|session| session.candidate_origin.clone())
            .unwrap_or_default()
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

    /// Poll inside a closed source/control loan without capturing traces or
    /// committing a new termination. The semantic boundary admits the returned
    /// error after releasing those loans, preserving complete suspended frames.
    pub(crate) fn poll_closed(&self) -> Result<(), RuntimeError> {
        self.execution.ensure_allowed()?;
        if let Some(session) = self.active_session() {
            if let Some(error) = session.termination.borrow().as_ref() {
                return Err(error.clone());
            }
            if session.options.cancellation.check().is_err() {
                return Err(CancellationReason::ExecutionCancelled.into());
            }
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
            RuntimeErrorKind::ResourceLimitExceeded
                | RuntimeErrorKind::StackOverflow
                | RuntimeErrorKind::AllocationFailure
                | RuntimeErrorKind::Cancelled
        ) && let Some(session) = self.active_session()
        {
            return session.terminate(self, error);
        }
        error
    }

    pub(crate) fn limit(&self, name: &'static str) -> RuntimeError {
        self.admit_execution_error(RuntimeError::resource_limit(name))
    }

    pub(crate) fn stack_overflow(&self) -> RuntimeError {
        self.admit_execution_error(RuntimeError::new(ErrorReason::StackOverflow))
    }

    pub(crate) fn allocation_failure(&self) -> RuntimeError {
        self.admit_execution_error(RuntimeError::new(ErrorReason::AllocationFailure))
    }

    pub(crate) fn close_execution(&self) {
        self.execution.close();
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

    pub(crate) fn quarantine(&self, reason: EngineFault) -> RuntimeError {
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

    pub(crate) fn stack_limit(&self) -> usize {
        self.limits.max_stack_bytes.get()
    }

    pub(crate) fn park_call_depth(&self) -> u32 {
        mem::take(&mut self.counters.borrow_mut().current_call_depth)
    }

    pub(crate) fn restore_call_depth(&self, depth: u32) {
        self.counters.borrow_mut().current_call_depth = depth;
    }

    pub(crate) fn enter_call(&self) -> Result<(), RuntimeError> {
        self.poll_execution()?;
        self.record_call()
            .map_err(|error| self.admit_execution_error(error))
    }

    /// Record an internal frame without trace capture under source/control loans.
    /// The closed executor admits any failure after releasing all those loans.
    pub(crate) fn enter_call_closed(&self) -> Result<(), RuntimeError> {
        self.poll_closed()?;
        self.record_call()
    }

    fn record_call(&self) -> Result<(), RuntimeError> {
        let mut counters = self.counters.borrow_mut();
        let next = counters
            .current_call_depth
            .checked_add(1)
            .ok_or_else(|| RuntimeError::new(ErrorReason::StackOverflow))?;
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
            self.quarantine(EngineFault::CallDepthUnderflowDuringFrameCleanup);
        }
    }

    /// Attribute a completed physical interval before its caller can execute
    /// callbacks, observe counters or switch sessions. The collector remains the
    /// sole authority for committed managed occupancy and intra-interval peaks.
    pub(crate) fn observe_heap(&self, report: AccountingReport) {
        if report.before == report.after && report.peak == report.before {
            return;
        }
        let temporary = self.temporary_heap_units.get() as u128;
        let peak = report.peak.units + temporary;
        let mut counters = self.counters.borrow_mut();
        counters.current_heap_units = report.after.units + temporary;
        counters.peak_heap_units = counters.peak_heap_units.max(peak);
        counters.current_retained_bytes = report.after.indirect_bytes as u128;
        counters.peak_retained_bytes = counters
            .peak_retained_bytes
            .max(report.peak.indirect_bytes as u128);
        if let Some(session) = self.active_session() {
            session
                .peak_heap_units
                .set(session.peak_heap_units.get().max(peak));
        }
    }

    /// Reserve before opening the physical interval that uses this scratch, and
    /// keep the guard until that interval closes. Its peak then overlaps every
    /// managed commit; reservation/polling must not run under physical loans.
    pub(crate) fn reserve_temporary_heap(
        &self,
        units: usize,
    ) -> Result<TemporaryHeap<'_>, RuntimeError> {
        self.poll_execution()?;
        let temporary = self
            .temporary_heap_units
            .get()
            .checked_add(units)
            .ok_or_else(|| self.limit("temporary heap units"))?;
        let mut counters = self.counters.borrow_mut();
        let current = counters.current_heap_units + units as u128;
        counters.current_heap_units = current;
        counters.peak_heap_units = counters.peak_heap_units.max(current);
        self.temporary_heap_units.set(temporary);
        if let Some(session) = self.active_session() {
            session
                .peak_heap_units
                .set(session.peak_heap_units.get().max(current));
        }
        Ok(TemporaryHeap {
            resources: self,
            units,
        })
    }

    fn release_temporary_heap(&self, units: usize) {
        let Some(temporary) = self.temporary_heap_units.get().checked_sub(units) else {
            self.quarantine(EngineFault::TemporaryHeapUnitsUnderflowDuringCleanup);
            return;
        };
        let mut counters = self.counters.borrow_mut();
        let Some(current) = counters.current_heap_units.checked_sub(units as u128) else {
            self.quarantine(EngineFault::TemporaryHeapUnitsUnderflowDuringCleanup);
            return;
        };
        self.temporary_heap_units.set(temporary);
        counters.current_heap_units = current;
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
        let resources = ResourceState::default();

        resources.enter_call().unwrap();
        resources.enter_call().unwrap();
        assert_eq!(resources.counters().peak_call_depth, 2);
        resources.enter_call().unwrap();
        assert_eq!(resources.counters().peak_call_depth, 3);
        resources.leave_call();
        assert_eq!(resources.counters().current_call_depth, 2);
    }
}

#[cfg(test)]
mod mutation_resources;
