use crate::{
    Runtime, StagedReload,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    frame::ExecutionFrame,
    gc::leases::LeaseScope,
    host::HostFrameId,
    module::{LoadedModule, retention::ProgramLease},
    resource::{ResourceCounters, ResourceState},
    session::store::SessionId,
    value::Value,
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use std::{
    any::Any,
    cell::{Cell, Ref, RefCell},
    collections::HashSet,
    fmt::Debug,
    time::Instant,
};

use kagari_bytecode::artifact::ArtifactFingerprint;

pub(crate) mod store;

/// Only checked closure handles authorize entering a retained program outside
/// the current dependency graph. Candidate entry remains independently gated.
pub(crate) enum ExecutionEntry {
    Program,
    Candidate,
    RetainedClosure,
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

impl TraceValue {
    fn capture(value: &Value, depth: usize, remaining: &mut usize) -> Self {
        if *remaining == 0 {
            return Self::Opaque("trace value budget".into());
        }
        *remaining -= 1;

        match value {
            Value::Range(value) => Self::Opaque(format!("{value:?}")),
            Value::Unit => Self::Unit,
            Value::Bool(value) => Self::Bool(*value),
            Value::I32(value) => Self::I32(*value),
            Value::I64(value) => Self::I64(*value),
            Value::U64(value) => Self::U64(*value),
            Value::F32(value) => Self::F32Bits(value.to_bits()),
            Value::F64(value) => Self::F64Bits(value.to_bits()),
            Value::Str(value) => Self::Str {
                prefix: value.chars().take(256).collect(),
                truncated: value.chars().nth(256).is_some(),
            },
            Value::Tuple(values) if depth < 4 => Self::Tuple {
                elements: values
                    .iter()
                    .take(16)
                    .map(|value| Self::capture(value, depth + 1, remaining))
                    .collect(),
                truncated: values.len() > 16,
            },
            Value::Tuple(_) => Self::Opaque("tuple depth limit".into()),
            Value::Array(id) => Self::Opaque(format!("array:{id:?}")),
            Value::Map(id) => Self::Opaque(format!("map:{id:?}")),
            Value::Set(id) => Self::Opaque(format!("set:{id:?}")),
            Value::Enum(id) => Self::Opaque(format!("enum:{id:?}")),
            Value::Struct(id) => Self::Opaque(format!("struct:{id:?}")),
            Value::GcHandle(id) => Self::Opaque(format!("gc:{id:?}")),
            Value::Interface(id) => Self::Opaque(format!("interface:{id:?}")),
            Value::Closure(id) => Self::Opaque(format!("closure:{id:?}")),
            Value::Cell(id) => Self::Opaque(format!("cell:{id:?}")),
            Value::HostRoot(_) => Self::Opaque("host root".into()),
            Value::HostPathView(_) => Self::Opaque("host path".into()),
            Value::Ephemeral(_) => Self::Opaque("ephemeral".into()),
        }
    }
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

/// Observers inspect the complete root stack between instructions. They must not
/// drive script execution while the frame view is borrowed.
/// Runtime exclusively owns their state; transfer requires Send, not Sync.
pub trait ExecutionObserver: Any + Debug + Send {
    /// Initialize observation once for this root session, before any frame runs.
    fn begin(&mut self, _runtime: &Runtime, _root: &LoadedModule) -> Result<(), RuntimeError> {
        Ok(())
    }

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
    pub leases: LeaseScope,
    pub host_scopes: RefCell<HashSet<HostFrameId>>,
    pub observer_attached: Cell<bool>,
    pub frame_scopes: RefCell<Vec<u64>>,
    pub next_frame_scope: Cell<u64>,
    pub scopes: Cell<usize>,
    pub peak_call_depth: Cell<u32>,
    pub peak_heap_units: Cell<usize>,
    pub root: LoadedModule,
    _program: ProgramLease,
    pub options: ExecutionOptions,
    pub termination: RefCell<Option<RuntimeError>>,
    random_counter: Cell<u64>,
    host_calls: RefCell<Vec<HostCallTrace>>,
    dropped_host_calls: Cell<usize>,
    started: Instant,
}

impl SessionState {
    pub(crate) fn new(
        id: SessionId,
        root: LoadedModule,
        options: ExecutionOptions,
        baseline: ResourceCounters,
        program: ProgramLease,
    ) -> Self {
        Self {
            id,
            leases: LeaseScope::default(),
            host_scopes: RefCell::new(HashSet::new()),
            observer_attached: Cell::new(false),
            frame_scopes: RefCell::new(Vec::new()),
            next_frame_scope: Cell::new(0),
            scopes: Cell::new(0),
            peak_call_depth: Cell::new(baseline.current_call_depth),
            peak_heap_units: Cell::new(baseline.current_heap_units),
            root,
            _program: program,
            options,
            termination: RefCell::new(None),
            random_counter: Cell::new(0),
            host_calls: RefCell::new(Vec::new()),
            dropped_host_calls: Cell::new(0),
            started: Instant::now(),
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

    pub(crate) fn begin_host_call(&self, symbol: &str, args: &[Value]) -> Option<usize> {
        if !self.options.record_host_calls {
            return None;
        }
        let mut calls = self.host_calls.borrow_mut();
        if calls.len() >= 10_000 {
            self.dropped_host_calls
                .set(self.dropped_host_calls.get().saturating_add(1));
            return None;
        }
        let index = calls.len();
        let mut remaining = 128;
        calls.push(HostCallTrace {
            symbol: symbol.chars().take(256).collect(),
            symbol_truncated: symbol.chars().nth(256).is_some(),
            arguments: args
                .iter()
                .take(32)
                .map(|value| TraceValue::capture(value, 0, &mut remaining))
                .collect(),
            omitted_arguments: args.len().saturating_sub(32),
            outcome: None,
        });
        Some(index)
    }

    pub(crate) fn finish_host_call(&self, index: usize, result: &Result<Value, RuntimeError>) {
        if let Some(call) = self.host_calls.borrow_mut().get_mut(index) {
            call.outcome = Some(match result {
                Ok(value) => Ok(TraceValue::capture(value, 0, &mut 128)),
                Err(error) => Err(error.kind()),
            });
        }
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
                RuntimeError::new(RuntimeErrorKind::Cancelled, "execution cancelled"),
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
/// use kagari_runtime::{Runtime, module::LoadedModule};
/// fn detach(runtime: Runtime, module: &LoadedModule) {
///     let session = runtime.begin_execution(module, runtime.execution_options()).unwrap();
///     drop(runtime);
///     drop(session);
/// }
/// ```
#[must_use]
pub struct ExecutionSession<'runtime> {
    pub(crate) id: SessionId,
    pub(crate) resources: &'runtime ResourceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionCounters {
    pub current_call_depth: u32,
    pub peak_call_depth: u32,
    pub current_heap_units: usize,
    pub peak_heap_units: usize,
    pub elapsed_wall_time_ms: u64,
}

impl ExecutionSession<'_> {
    pub(crate) fn state(&self) -> Ref<'_, SessionState> {
        self.resources
            .sessions
            .get(self.id)
            .expect("live execution scope")
    }

    pub fn trace(&self) -> Option<ExecutionTrace> {
        let state = self.state();
        state.options.record_host_calls.then(|| ExecutionTrace {
            root_identity: state.root.bytecode.identity.clone(),
            code_fingerprint: state.root.program_fingerprint(),
            inputs: state.options.inputs,
            host_calls: state.host_calls.borrow().clone(),
            dropped_host_calls: state.dropped_host_calls.get(),
        })
    }

    pub fn host_scope_count(&self) -> usize {
        let state = self.state();
        state.host_scopes.borrow().len()
    }

    pub fn root(&self) -> LoadedModule {
        let state = self.state();
        state.root.clone()
    }

    pub fn counters(&self) -> ExecutionCounters {
        let state = self.state();
        let counters = self.resources.counters();
        ExecutionCounters {
            current_call_depth: counters.current_call_depth,
            peak_call_depth: state.peak_call_depth.get(),
            current_heap_units: counters.current_heap_units,
            peak_heap_units: state.peak_heap_units.get(),
            elapsed_wall_time_ms: state
                .started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        }
    }
}

impl Drop for ExecutionSession<'_> {
    fn drop(&mut self) {
        let state = self.state();
        let scopes = state.scopes.get();
        state.scopes.set(scopes - 1);
        if scopes == 1 {
            if self
                .resources
                .sessions
                .frames(self.id)
                .is_none_or(|frames| !frames.is_empty())
                || !state.frame_scopes.borrow().is_empty()
                || !state.host_scopes.borrow().is_empty()
            {
                self.resources
                    .quarantine("execution session ended with active resources");
            }
            self.resources.end_execution(self.id);
            drop(state);
            if self.resources.sessions.remove(self.id).is_none() {
                self.resources
                    .quarantine("session records remained borrowed during cleanup");
            }
        }
    }
}

/// A separate initialization root that restores a suspended ordinary call on exit.
pub struct CandidateSession<'runtime, 'candidate> {
    pub(crate) candidate: &'candidate StagedReload,
    pub(crate) execution: Option<ExecutionSession<'runtime>>,
    pub(crate) previous: Option<SessionId>,
    pub(crate) resources: &'runtime ResourceState,
}

impl Drop for CandidateSession<'_, '_> {
    fn drop(&mut self) {
        if let Some(execution) = &self.execution
            && let Err(error) = execution.state().poll(self.resources)
        {
            self.candidate.record_initialization_error(error);
        }
        if self
            .execution
            .as_ref()
            .is_some_and(|execution| execution.state().scopes.get() != 1)
        {
            self.resources
                .quarantine("candidate session ended with nested execution scopes");
        }
        drop(self.execution.take());
        let previous = self.previous.take().filter(|id| {
            self.resources
                .sessions
                .get(*id)
                .is_some_and(|state| state.scopes.get() != 0)
        });
        self.resources.replace_session(previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;
    use kagari_bytecode::{
        module::BytecodeModule,
        program::{BytecodeProgram, ModuleRef},
    };

    #[test]
    fn trace_values_report_truncation_and_stop_at_a_shared_budget() {
        let mut budget = 128;
        assert_eq!(
            TraceValue::capture(&Value::Str("x".repeat(300)), 0, &mut budget),
            TraceValue::Str {
                prefix: "x".repeat(256),
                truncated: true,
            }
        );
        let mut budget = 3;
        let captured = TraceValue::capture(&Value::Tuple(vec![Value::I32(1); 20]), 0, &mut budget);
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
        assert!(elements[2..].iter().all(
            |value| matches!(value, TraceValue::Opaque(reason) if reason == "trace value budget")
        ));
    }

    #[test]
    fn trace_call_limit_reports_omitted_invocations() {
        let mut runtime = crate::Runtime::default();
        let module = runtime
            .load_program(
                "trace-cap",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
            )
            .unwrap();
        let mut options = runtime.execution_options();
        options.record_host_calls = true;
        let session = runtime.begin_execution(&module, options).unwrap();
        for _ in 0..10_001 {
            session.state().begin_host_call("ping", &[]);
        }
        let trace = session.trace().unwrap();
        assert_eq!(trace.host_calls.len(), 10_000);
        assert_eq!(trace.dropped_host_calls, 1);
    }
}
