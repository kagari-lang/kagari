use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};

use kagari_common::cancellation::CancellationToken;

use crate::{
    HostExposurePolicy, LoadedModule, ModuleEpochRetention, ModuleStore, ResourceCounters,
    ResourcePolicy, ResourceState, RuntimeError, RuntimeErrorKind, SecurityContext,
};

/// Restrictions attached to the root session and inherited by synchronous reentry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExecutionPhase {
    #[default]
    Ordinary,
    CandidateInitialization,
}

/// Host-selected inputs for a root call. Nested entries inherit the active inputs.
#[derive(Debug, Clone, Default)]
pub struct ExecutionOptions {
    pub phase: ExecutionPhase,
    pub security: SecurityContext,
    pub host_exposure: Rc<HostExposurePolicy>,
    pub resources: ResourcePolicy,
    pub cancellation: CancellationToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionEvent {
    BeforeInstruction,
    Trap,
}

/// Observers inspect the complete root stack between instructions. They must not
/// drive script execution while the frame view is borrowed.
pub trait ExecutionObserver: std::fmt::Debug {
    fn observe(
        &self,
        runtime: &crate::Runtime,
        event: ExecutionEvent,
        frames: &[crate::ExecutionFrame],
    ) -> Result<(), RuntimeError>;
}

#[derive(Debug)]
pub(crate) struct SessionState {
    pub host_scopes: RefCell<
        std::collections::HashMap<crate::HostFrameId, Rc<crate::host_scope::HostScopeState>>,
    >,
    pub observer: RefCell<Option<Rc<dyn ExecutionObserver>>>,
    pub frames: RefCell<Vec<crate::ExecutionFrame>>,
    pub frame_scopes: RefCell<Vec<u64>>,
    pub next_frame_scope: Cell<u64>,
    pub scopes: Cell<usize>,
    pub peak_call_depth: Cell<u32>,
    pub peak_heap_units: Cell<usize>,
    pub root: LoadedModule,
    pub options: ExecutionOptions,
    pub baseline: ResourceCounters,
    pub termination: RefCell<Option<RuntimeError>>,
    started: Instant,
}

impl SessionState {
    pub(crate) fn new(
        root: LoadedModule,
        options: ExecutionOptions,
        baseline: ResourceCounters,
    ) -> Self {
        Self {
            host_scopes: RefCell::new(std::collections::HashMap::new()),
            observer: RefCell::new(None),
            frames: RefCell::new(Vec::new()),
            frame_scopes: RefCell::new(Vec::new()),
            next_frame_scope: Cell::new(0),
            scopes: Cell::new(0),
            peak_call_depth: Cell::new(baseline.current_call_depth),
            peak_heap_units: Cell::new(baseline.current_heap_units),
            root,
            options,
            baseline,
            termination: RefCell::new(None),
            started: Instant::now(),
        }
    }

    pub(crate) fn terminate(&self, error: RuntimeError) -> RuntimeError {
        self.termination.borrow_mut().get_or_insert(error).clone()
    }

    pub(crate) fn poll(&self) -> Result<(), RuntimeError> {
        if let Some(error) = self.termination.borrow().as_ref() {
            return Err(error.clone());
        }
        if self.options.cancellation.check().is_err() {
            return Err(self.terminate(RuntimeError::new(
                RuntimeErrorKind::Cancelled,
                "execution cancelled",
            )));
        }
        if self
            .options
            .resources
            .max_wall_time_ms
            .is_some_and(|limit| self.started.elapsed().as_millis() >= u128::from(limit))
        {
            return Err(self.terminate(RuntimeError::resource_limit("wall time")));
        }
        Ok(())
    }
}

/// An owned scope, so driving the VM does not borrow the runtime for the call.
/// Last scope drop releases the pinned program and active execution inputs.
#[must_use]
pub struct ExecutionSession {
    pub(crate) state: Rc<SessionState>,
    pub(crate) resources: Rc<ResourceState>,
    pub(crate) modules: ModuleStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionCounters {
    pub instruction_steps: u64,
    pub allocation_units: usize,
    pub host_calls: u64,
    pub reflection_operations: u64,
    pub current_call_depth: u32,
    pub peak_call_depth: u32,
    pub current_heap_units: usize,
    pub peak_heap_units: usize,
    pub elapsed_wall_time_ms: u64,
}

impl ExecutionSession {
    pub fn host_scope_count(&self) -> usize {
        self.state.host_scopes.borrow().len()
    }
    pub fn root(&self) -> &LoadedModule {
        &self.state.root
    }
    pub fn counters(&self) -> ExecutionCounters {
        let counters = self.resources.counters();
        let baseline = self.state.baseline;
        ExecutionCounters {
            instruction_steps: counters.instruction_steps - baseline.instruction_steps,
            allocation_units: counters.allocation_units - baseline.allocation_units,
            host_calls: counters.host_calls - baseline.host_calls,
            reflection_operations: counters.reflection_operations - baseline.reflection_operations,
            current_call_depth: counters.current_call_depth,
            peak_call_depth: self.state.peak_call_depth.get(),
            current_heap_units: counters.current_heap_units,
            peak_heap_units: self.state.peak_heap_units.get(),
            elapsed_wall_time_ms: self
                .state
                .started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        }
    }
}

impl Drop for ExecutionSession {
    fn drop(&mut self) {
        let scopes = self.state.scopes.get();
        self.state.scopes.set(scopes - 1);
        if scopes == 1 {
            if !self.state.frames.borrow().is_empty()
                || !self.state.frame_scopes.borrow().is_empty()
                || !self.state.host_scopes.borrow().is_empty()
            {
                self.resources
                    .quarantine("execution session ended with active resources");
            }
            self.resources.end_execution(&self.state);
            self.modules
                .release_epoch(self.state.root.key(), ModuleEpochRetention::ActiveCall);
        }
    }
}

/// A separate initialization root that restores a suspended ordinary call on exit.
pub struct CandidateSession<'candidate> {
    pub(crate) candidate: &'candidate crate::StagedReload,
    pub(crate) execution: Option<ExecutionSession>,
    pub(crate) previous: Option<Rc<SessionState>>,
    pub(crate) resources: Rc<ResourceState>,
}
impl Drop for CandidateSession<'_> {
    fn drop(&mut self) {
        if let Some(execution) = &self.execution
            && let Err(error) = execution.state.poll()
        {
            self.candidate.record_initialization_error(error);
        }
        if self
            .execution
            .as_ref()
            .is_some_and(|execution| execution.state.scopes.get() != 1)
        {
            self.resources
                .quarantine("candidate session ended with nested execution scopes");
        }
        drop(self.execution.take());
        let previous = self.previous.take().filter(|state| state.scopes.get() != 0);
        self.resources.replace_session(previous);
    }
}
