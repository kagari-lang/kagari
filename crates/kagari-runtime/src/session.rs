use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};

use kagari_common::cancellation::CancellationToken;
use kagari_ir::bytecode::ArtifactFingerprint;

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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeterministicInputs {
    pub unix_time_millis: i64,
    pub random_seed: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionOptions {
    pub phase: ExecutionPhase,
    pub security: SecurityContext,
    pub host_exposure: Rc<HostExposurePolicy>,
    pub resources: ResourcePolicy,
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
    fn capture(value: &crate::value::Value, depth: usize, remaining: &mut usize) -> Self {
        if *remaining == 0 {
            return Self::Opaque("trace value budget".into());
        }
        *remaining -= 1;
        use crate::value::Value;
        match value {
            Value::Unit => Self::Unit,
            Value::Bool(value) => Self::Bool(*value),
            Value::I32(value) => Self::I32(*value),
            Value::I64(value) => Self::I64(*value),
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
    pub root_identity: kagari_common::identity::ModuleIdentity,
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
    random_counter: Cell<u64>,
    host_calls: RefCell<Vec<HostCallTrace>>,
    dropped_host_calls: Cell<usize>,
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

    pub(crate) fn begin_host_call(
        &self,
        symbol: &str,
        args: &[crate::value::Value],
    ) -> Option<usize> {
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

    pub(crate) fn finish_host_call(
        &self,
        index: usize,
        result: &Result<crate::value::Value, RuntimeError>,
    ) {
        if let Some(call) = self.host_calls.borrow_mut().get_mut(index) {
            call.outcome = Some(match result {
                Ok(value) => Ok(TraceValue::capture(value, 0, &mut 128)),
                Err(error) => Err(error.kind()),
            });
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
    pub fn trace(&self) -> Option<ExecutionTrace> {
        self.state
            .options
            .record_host_calls
            .then(|| ExecutionTrace {
                root_identity: self.state.root.bytecode.identity.clone(),
                code_fingerprint: self.state.root.program_fingerprint(),
                inputs: self.state.options.inputs,
                host_calls: self.state.host_calls.borrow().clone(),
                dropped_host_calls: self.state.dropped_host_calls.get(),
            })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;
    use kagari_ir::bytecode::{BytecodeModule, BytecodeProgram, ModuleRef};

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
            session.state.begin_host_call("ping", &[]);
        }
        let trace = session.trace().unwrap();
        assert_eq!(trace.host_calls.len(), 10_000);
        assert_eq!(trace.dropped_host_calls, 1);
    }
}
