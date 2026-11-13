#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime, StagedReload,
    error::{RuntimeError, RuntimeErrorKind, fault::EngineFault, reason::CancellationReason},
    error_trace::ErrorTrace,
    execution::{
        memory::ManagedMemory,
        roots::{DependencyId, SessionId},
    },
    frame::{
        ExecutionFrame,
        factory::QueuedFactory,
        types::arguments::TypeArgument,
        waiting::{PendingWait, QueuedFuture},
    },
    host::HostFrameId,
    module::{
        ExecutableModule, managed::revision::RevisionState, retention::RetentionRegistration,
    },
    publication::ActivationMark,
    resource::{ResourceCounters, ResourceState, candidate::CandidateOrigin},
};

use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use std::{
    any::Any,
    cell::{Cell, Ref, RefCell},
    collections::HashSet,
    fmt::Debug,
    sync::{Arc, atomic::AtomicUsize},
};

use kagari_bytecode::artifact::ArtifactFingerprint;

pub(crate) mod dependencies;
pub mod group;
pub mod owned;
pub(crate) mod store;
mod tracing;

/// Only checked callable handles authorize entering a retained program outside
/// the current dependency graph. Candidate entry remains independently gated.
pub(crate) enum ExecutionEntry {
    Program,
    Candidate,
    RetainedClosure,
    RetainedFunction,
}

/// Restrictions attached to the root session and inherited by synchronous reentry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExecutionPhase {
    #[default]
    Ordinary,
    CandidateInitialization,
}

/// Host-selected inputs for a root call. Nested entries inherit the active inputs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeterministicInputs {
    pub unix_time_millis: i64,
    pub random_seed: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionOptions {
    pub phase: ExecutionPhase,

    pub cancellation: CancellationToken,
    pub inputs: DeterministicInputs,
    pub record_host_calls: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceValue {
    Unit,
    Bool(bool),
    Char(char),
    I32(i32),
    I64(i64),
    U64(u64),
    F32Bits(u32),
    F64Bits(u64),
    Str {
        prefix: String,
        truncated: bool,
    },
    Tuple {
        elements: Vec<Self>,
        truncated: bool,
    },
    Opaque(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCallTrace {
    pub symbol: String,
    pub symbol_truncated: bool,
    pub arguments: Vec<TraceValue>,
    pub omitted_arguments: usize,
    pub outcome: Option<Result<TraceValue, RuntimeErrorKind>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionTrace {
    pub root_identity: ModuleIdentity,
    pub code_fingerprint: ArtifactFingerprint,
    pub inputs: DeterministicInputs,
    pub host_calls: Vec<HostCallTrace>,
    pub dropped_host_calls: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionEvent {
    BeforeInstruction,
    Trap,
}

/// Detached root identity for diagnostics and debugger selection, not entry authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutionId(pub(crate) SessionId);

/// Observers inspect the complete root stack between instructions. They must not
/// drive script execution while the frame view is borrowed.
/// Runtime exclusively owns their state; transfer requires Send, not Sync.
pub trait ExecutionObserver: Any + Debug + Send {
    /// Initialize observation once for this root session, before any frame runs.
    fn begin(&mut self, _runtime: &Runtime, _root: &ExecutableModule) -> Result<(), RuntimeError> {
        Ok(())
    }

    /// Decide whether this event needs retained live bindings before observation.
    /// The default performs no value capture. Preparation may update observer
    /// policy, but receives no mutable execution services. Runtime calls observe
    /// immediately after capture without execution, publication or collection.
    fn capture_binding_values(
        &mut self,
        _runtime: &Runtime,
        _event: ExecutionEvent,
        _frames: &[ExecutionFrame],
    ) -> bool {
        false
    }

    /// Inspect without lending mutable execution services. Reentry is excluded
    /// while the observer's frame slice is borrowed:
    /// ```compile_fail,E0596
    /// use kagari_runtime::{Runtime, error::RuntimeError, frame::ExecutionFrame,
    ///     module::entry::ProgramEntry, session::{ExecutionEvent, ExecutionObserver}};
    /// #[derive(Debug)]
    /// struct Reentrant;
    /// impl ExecutionObserver for Reentrant {
    ///     fn observe(&mut self, runtime: &Runtime, _: ExecutionEvent,
    ///                frames: &[ExecutionFrame]) -> Result<(), RuntimeError> {
    ///         if let Some(frame) = frames.last() {
    ///             runtime.execute_module(frame.loaded(), ProgramEntry::Main,
    ///                                    runtime.execution_options())?;
    ///         }
    ///         Ok(())
    ///     }
    /// }
    /// ```
    fn observe(
        &mut self,
        runtime: &Runtime,
        event: ExecutionEvent,
        frames: &[ExecutionFrame],
    ) -> Result<(), RuntimeError>;
}

#[derive(Debug)]
pub(crate) struct SessionState {
    pub id: SessionId,
    pub owner: RefCell<Option<Arc<owned::ExecutionOwner>>>,
    pub parked_depth: Cell<u32>,
    pub depth_base: Cell<u32>,
    pub group_depth: Cell<usize>,
    pub logical_root: Cell<SessionId>,
    pub group_parent: Cell<Option<SessionId>>,
    pub static_initializers: Arc<AtomicUsize>,
    pub pending: RefCell<Option<PendingWait>>,
    pub queued_future: RefCell<Option<QueuedFuture>>,
    pub queued_factory: RefCell<Option<QueuedFactory>>,
    pub factory_output: RefCell<Option<TypeArgument>>,
    pub host_scopes: RefCell<HashSet<HostFrameId>>,
    pub observer_attached: Cell<bool>,
    pub frame_scopes: RefCell<Vec<u64>>,
    pub next_frame_scope: Cell<u64>,
    pub scopes: Cell<usize>,
    pub peak_call_depth: Cell<u32>,
    pub peak_heap_units: Cell<u128>,
    pub root: ExecutableModule,
    pub(crate) candidate_origin: CandidateOrigin,
    pub(crate) body: DependencyId<RevisionState>,
    _registration: RetentionRegistration,
    pub options: ExecutionOptions,
    pub termination: RefCell<Option<RuntimeError>>,
    random_counter: Cell<u64>,
    host_calls: RefCell<Vec<HostCallTrace>>,
    dropped_host_calls: Cell<usize>,
}

impl SessionState {
    pub(crate) fn new(
        id: SessionId,
        root: ExecutableModule,
        options: ExecutionOptions,
        baseline: ResourceCounters,
        registration: RetentionRegistration,
        body: DependencyId<RevisionState>,
    ) -> Self {
        Self {
            id,
            body,
            owner: RefCell::new(None),
            parked_depth: Cell::new(0),
            depth_base: Cell::new(0),
            group_depth: Cell::new(0),
            logical_root: Cell::new(id),
            group_parent: Cell::new(None),
            static_initializers: Arc::new(AtomicUsize::new(0)),
            pending: RefCell::new(None),
            queued_future: RefCell::new(None),
            queued_factory: RefCell::new(None),
            factory_output: RefCell::new(None),
            host_scopes: RefCell::new(HashSet::new()),
            observer_attached: Cell::new(false),
            frame_scopes: RefCell::new(Vec::new()),
            next_frame_scope: Cell::new(0),
            scopes: Cell::new(0),
            peak_call_depth: Cell::new(baseline.current_call_depth),
            peak_heap_units: Cell::new(baseline.current_heap_units),
            candidate_origin: if options.phase == ExecutionPhase::CandidateInitialization {
                CandidateOrigin::new(root.program_identity())
            } else {
                CandidateOrigin::default()
            },
            root,
            _registration: registration,
            options,
            termination: RefCell::new(None),
            random_counter: Cell::new(0),
            host_calls: RefCell::new(Vec::new()),
            dropped_host_calls: Cell::new(0),
        }
    }

    pub(crate) fn next_random_u64(&self) -> u64 {
        // SplitMix64 gives a stable stream from the host-provided seed.
        let counter = self.random_counter.get();
        self.random_counter.set(counter.wrapping_add(1));
        let mut value = self
            .options
            .inputs
            .random_seed
            .wrapping_add(counter.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    pub(crate) fn terminate(&self, resources: &ResourceState, error: RuntimeError) -> RuntimeError {
        let error = error.with_trace(ErrorTrace::capture_session(resources, self.id));
        self.termination.borrow_mut().get_or_insert(error).clone()
    }

    pub(crate) fn poll(&self, resources: &ResourceState) -> Result<(), RuntimeError> {
        if let Some(error) = self.termination.borrow().as_ref() {
            return Err(error.clone());
        }
        if self.options.cancellation.check().is_err() {
            return Err(self.terminate(
                resources,
                RuntimeError::from(CancellationReason::ExecutionCancelled),
            ));
        }
        Ok(())
    }
}

/// A scope borrowing runtime-owned session storage.
/// Last scope drop releases the pinned program and active execution inputs.
/// The runtime cannot be moved or destroyed while a scope is live.
///
/// ```compile_fail
/// use kagari_runtime::{Runtime, module::ExecutableModule};
/// fn detach(mut runtime: Runtime, module: &ExecutableModule) {
///     let session = runtime.begin_execution(module, runtime.execution_options()).unwrap();
///     drop(runtime);
///     drop(session);
/// }
/// ```
/// Active admission cannot move to another thread:
/// ```compile_fail,E0277
/// use kagari_runtime::{Runtime, module::ExecutableModule};
/// fn transfer(runtime: &mut Runtime, module: &ExecutableModule) {
///     let session = runtime.begin_execution(module, Default::default()).unwrap();
///     std::thread::scope(|threads| { threads.spawn(move || drop(session)); });
/// }
/// ```
#[must_use]
pub struct ExecutionSession<'runtime> {
    pub(crate) runtime: &'runtime mut Runtime,
    pub(crate) scope: SessionScope,
}

/// Session identity and cleanup authority. No heap or Runtime borrow is retained
/// in this record; the owning activation supplies its service access on exit.
pub(crate) struct SessionScope {
    pub(crate) id: SessionId,
    activation: Option<ActivationMark>,
}

impl SessionScope {
    pub(crate) fn new(id: SessionId, activation: ActivationMark) -> Self {
        Self {
            id,
            activation: Some(activation),
        }
    }

    pub(crate) fn trace(&self, resources: &ResourceState) -> Option<ExecutionTrace> {
        let state = self.state(resources);
        state.options.record_host_calls.then(|| ExecutionTrace {
            root_identity: state.root.bytecode.identity.clone(),
            code_fingerprint: state.root.program_fingerprint(),
            inputs: state.options.inputs,
            host_calls: state.host_calls.borrow().clone(),
            dropped_host_calls: state.dropped_host_calls.get(),
        })
    }

    pub(crate) fn state<'r>(&self, resources: &'r ResourceState) -> Ref<'r, SessionState> {
        resources
            .sessions
            .get(self.id)
            .expect("live execution scope")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionCounters {
    pub current_call_depth: u32,
    pub peak_call_depth: u32,
    pub current_heap_units: u128,
    pub peak_heap_units: u128,
}

impl ExecutionSession<'_> {
    /// Inspect the admitted Runtime without driving it.
    pub fn runtime(&self) -> &Runtime {
        self.runtime
    }

    /// Reborrow the same Runtime services for a nested synchronous operation.
    /// The session retains admission and cleanup authority across that operation.
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.runtime
    }

    pub(crate) fn state(&self) -> Ref<'_, SessionState> {
        self.scope.state(self.runtime.resources())
    }

    pub fn trace(&self) -> Option<ExecutionTrace> {
        self.scope.trace(self.runtime.resources())
    }

    pub fn host_scope_count(&self) -> usize {
        let state = self.state();
        state.host_scopes.borrow().len()
    }

    pub fn root(&self) -> ExecutableModule {
        let state = self.state();
        state.root.clone()
    }

    pub fn counters(&self) -> ExecutionCounters {
        let state = self.state();
        let counters = self.runtime.resources().counters();
        ExecutionCounters {
            current_call_depth: counters.current_call_depth,
            peak_call_depth: state.peak_call_depth.get(),
            current_heap_units: counters.current_heap_units,
            peak_heap_units: state.peak_heap_units.get(),
        }
    }
}

impl SessionScope {
    fn release(&self, resources: &ResourceState, memory: &mut ManagedMemory) {
        let state = self.state(resources);
        let scopes = state.scopes.get();
        state.scopes.set(scopes - 1);
        if scopes == 1 {
            if state.owner.borrow().is_some() {
                if !state.frame_scopes.borrow().is_empty() || !state.host_scopes.borrow().is_empty()
                {
                    resources.quarantine(EngineFault::OwnedActivationEndedWithActiveScopes);
                }
                state.parked_depth.set(resources.park_call_depth());
                resources.end_execution(self.id);
                return;
            }
            if resources
                .sessions
                .frames(self.id)
                .is_none_or(|frames| !frames.is_empty())
                || !state.frame_scopes.borrow().is_empty()
                || !state.host_scopes.borrow().is_empty()
            {
                resources.quarantine(EngineFault::ExecutionSessionEndedWithActiveResources);
            }
            resources.end_execution(self.id);
            drop(state);
            if let Some((_state, frames)) = resources.sessions.remove(self.id) {
                // Retire before dropping detached Rust owners, including unwind.
                // No user callback or collection intervenes in this closed handoff.
                if let Err(error) = memory.retire_execution(resources, self.id)
                    && !resources.is_quarantined()
                {
                    resources.quarantine(EngineFault::ExecutionStorageRetirementFailed(error));
                }
                // The whole-execution owner retired every frame/local suffix.
                // Detached controls contain no additional managed storage to retire.
                drop(frames);
            } else {
                resources.quarantine(EngineFault::SessionRecordsRemainedBorrowedDuringCleanup);
            }
        }
    }
}

impl SessionScope {
    pub(crate) fn retire(&mut self, runtime: &mut Runtime) {
        if let Some(activation) = self.activation.take() {
            let resources = &runtime.resources;
            let _activation = activation.attach(resources);
            self.release(resources, &mut runtime.memory);
        }
    }
}

impl Drop for ExecutionSession<'_> {
    fn drop(&mut self) {
        self.scope.retire(self.runtime);
    }
}

/// A separate initialization root that restores a suspended ordinary call on exit.
pub struct CandidateSession<'runtime, 'candidate> {
    pub(crate) candidate: &'candidate StagedReload,
    pub(crate) runtime: &'runtime mut Runtime,
    pub(crate) execution: Option<SessionScope>,
    pub(crate) previous: Option<SessionId>,
}

impl CandidateSession<'_, '_> {
    /// Reborrow the isolated candidate's services. Ordinary publication remains
    /// excluded until this guard retires candidate state and restores its parent.
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.runtime
    }
}

impl Drop for CandidateSession<'_, '_> {
    fn drop(&mut self) {
        let resources = self.runtime.resources();
        if let Some(execution) = &self.execution
            && let Err(error) = execution.state(resources).poll(resources)
        {
            self.candidate.record_initialization_error(error);
        }
        if self
            .execution
            .as_ref()
            .is_some_and(|execution| execution.state(resources).scopes.get() != 1)
        {
            resources.quarantine(EngineFault::CandidateSessionEndedWithNestedExecutionScopes);
        }
        if let Some(mut execution) = self.execution.take() {
            execution.retire(self.runtime);
        }
        let resources = self.runtime.resources();
        let previous = self.previous.take().filter(|id| {
            resources
                .sessions
                .get(*id)
                .is_some_and(|state| state.scopes.get() != 0)
        });
        resources.replace_session(previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::native::program::ProgramFixture;
    use crate::value::{
        aggregates::tuples::TupleObject, carriers::ScopedValue, strings::StringObject,
    };
    use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
    use kagari_common::identity::map::DefinitionContext;

    #[test]
    fn trace_values_report_truncation_and_stop_at_a_shared_budget() {
        let mut runtime = Runtime::default();
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let text = StringObject::alloc(cx, "x".repeat(300))?;
                let captured = TraceValue::capture(cx, ScopedValue::managed(text), 0, &mut 128)?;
                assert_eq!(
                    captured,
                    TraceValue::Str {
                        prefix: "x".repeat(256),
                        truncated: true,
                    }
                );
                let tuple = TupleObject::alloc(cx, &[ScopedValue::from(1_i32); 20])?;
                let captured = TraceValue::capture(cx, ScopedValue::managed(tuple), 0, &mut 3)?;
                let TraceValue::Tuple {
                    elements,
                    truncated,
                } = captured
                else {
                    panic!("expected tuple trace");
                };
                assert!(truncated);
                assert_eq!(elements.len(), 16);
                assert_eq!(&elements[..2], &[TraceValue::I32(1), TraceValue::I32(1)]);
                assert!(elements[2..].iter().all(|value| matches!(
                    value,
                    TraceValue::Opaque(reason) if reason == "trace value budget"
                )));
                Ok(())
            })
            .unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, 0);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().objects, 0);
    }

    #[test]
    fn trace_call_limit_reports_omitted_invocations() {
        let context = DefinitionContext::new().unwrap();
        let mut runtime = crate::Runtime::default();
        let program = load_bytecode(
            &mut runtime,
            "trace-cap",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
        let module = runtime.select_program(&program).unwrap();
        let mut options = runtime.execution_options();
        options.record_host_calls = true;
        let mut session = runtime.begin_execution(&module, options).unwrap();
        for _ in 0..10_001 {
            session
                .runtime_mut()
                .begin_host_call_trace(|| Ok("ping".into()), &[])
                .unwrap();
        }
        let trace = session.trace().unwrap();
        assert_eq!(trace.host_calls.len(), 10_000);
        assert_eq!(trace.dropped_host_calls, 1);
    }
}

#[cfg(test)]
mod execution_sessions;
