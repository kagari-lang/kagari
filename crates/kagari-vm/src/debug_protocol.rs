use kagari_runtime::value::Value;

use crate::{
    BreakpointId, DebugFrameId, DebugPause, DebugSession, DebugWatch, ResolvedBreakpoint,
    SourceBreakpoint, Vm, VmError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugAdapterCapabilities {
    pub source_breakpoints: bool,
    pub pause: bool,
    pub stepping: bool,
    pub run_to_cursor: bool,
    pub stack_inspection: bool,
    pub watch_evaluation: bool,
}

impl Default for DebugAdapterCapabilities {
    fn default() -> Self {
        Self {
            source_breakpoints: true,
            pause: true,
            stepping: true,
            run_to_cursor: true,
            stack_inspection: true,
            watch_evaluation: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugAdapterRequest {
    Attach,
    SetBreakpoint(SourceBreakpoint),
    Continue,
    Pause,
    StepInto,
    StepOver {
        frame_depth: usize,
    },
    StepOut {
        frame_depth: usize,
    },
    RunToCursor {
        source_uri: String,
        source_offset: usize,
    },
    EvaluateWatch {
        pause_index: usize,
        frame_id: DebugFrameId,
        watch: DebugWatch,
    },
    FlushEvents,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DebugAdapterResponse {
    Attached {
        capabilities: DebugAdapterCapabilities,
    },
    BreakpointSet {
        breakpoint_id: BreakpointId,
    },
    Continued,
    PauseRequested,
    StepConfigured,
    WatchValue {
        value: Value,
    },
    EventsFlushed {
        emitted: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DebugAdapterEvent {
    Attached {
        capabilities: DebugAdapterCapabilities,
    },
    BreakpointSet {
        breakpoint_id: BreakpointId,
    },
    BreakpointResolved(ResolvedBreakpoint),
    Continued,
    PauseRequested,
    Paused(DebugPause),
}

pub trait DebugAdapterEventSink {
    fn emit(&mut self, event: DebugAdapterEvent);
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct RecordingDebugAdapterSink {
    events: Vec<DebugAdapterEvent>,
}

impl RecordingDebugAdapterSink {
    pub fn events(&self) -> &[DebugAdapterEvent] {
        &self.events
    }
}

impl DebugAdapterEventSink for RecordingDebugAdapterSink {
    fn emit(&mut self, event: DebugAdapterEvent) {
        self.events.push(event);
    }
}

#[derive(Debug, Clone)]
pub struct DebugProtocolAdapter<S> {
    sink: S,
    emitted_resolved_breakpoints: usize,
    emitted_pauses: usize,
    capabilities: DebugAdapterCapabilities,
}

impl DebugProtocolAdapter<RecordingDebugAdapterSink> {
    pub fn recording() -> Self {
        Self::new(RecordingDebugAdapterSink::default())
    }
}

impl<S> DebugProtocolAdapter<S> {
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            emitted_resolved_breakpoints: 0,
            emitted_pauses: 0,
            capabilities: DebugAdapterCapabilities::default(),
        }
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }

    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    pub fn into_sink(self) -> S {
        self.sink
    }
}

impl<S: DebugAdapterEventSink> DebugProtocolAdapter<S> {
    pub fn handle_request(
        &mut self,
        vm: &mut Vm,
        request: DebugAdapterRequest,
    ) -> Result<DebugAdapterResponse, VmError> {
        match request {
            DebugAdapterRequest::Attach => self.attach(vm),
            DebugAdapterRequest::SetBreakpoint(breakpoint) => self.set_breakpoint(vm, breakpoint),
            DebugAdapterRequest::Continue => {
                session_mut(vm)?.continue_execution()?;
                self.sink.emit(DebugAdapterEvent::Continued);
                Ok(DebugAdapterResponse::Continued)
            }
            DebugAdapterRequest::Pause => {
                session_mut(vm)?.pause()?;
                self.sink.emit(DebugAdapterEvent::PauseRequested);
                Ok(DebugAdapterResponse::PauseRequested)
            }
            DebugAdapterRequest::StepInto => {
                session_mut(vm)?.step_into()?;
                Ok(DebugAdapterResponse::StepConfigured)
            }
            DebugAdapterRequest::StepOver { frame_depth } => {
                session_mut(vm)?.step_over(frame_depth)?;
                Ok(DebugAdapterResponse::StepConfigured)
            }
            DebugAdapterRequest::StepOut { frame_depth } => {
                session_mut(vm)?.step_out(frame_depth)?;
                Ok(DebugAdapterResponse::StepConfigured)
            }
            DebugAdapterRequest::RunToCursor {
                source_uri,
                source_offset,
            } => {
                let breakpoint_id = session_mut(vm)?.run_to_cursor(source_uri, source_offset)?;
                self.sink
                    .emit(DebugAdapterEvent::BreakpointSet { breakpoint_id });
                Ok(DebugAdapterResponse::BreakpointSet { breakpoint_id })
            }
            DebugAdapterRequest::EvaluateWatch {
                pause_index,
                frame_id,
                watch,
            } => {
                let value = session(vm)?
                    .pauses()
                    .get(pause_index)
                    .ok_or(VmError::UnsupportedInstruction("debug_pause_index"))?
                    .evaluate_watch(vm.runtime(), frame_id, &watch)?;
                Ok(DebugAdapterResponse::WatchValue { value })
            }
            DebugAdapterRequest::FlushEvents => {
                let emitted = self.flush_events(vm)?;
                Ok(DebugAdapterResponse::EventsFlushed { emitted })
            }
        }
    }

    pub fn flush_events(&mut self, vm: &Vm) -> Result<usize, VmError> {
        let session = session(vm)?;
        let resolved = session
            .resolved_breakpoints()
            .iter()
            .skip(self.emitted_resolved_breakpoints)
            .cloned()
            .collect::<Vec<_>>();
        let pauses = session
            .pauses()
            .iter()
            .skip(self.emitted_pauses)
            .cloned()
            .collect::<Vec<_>>();
        self.emitted_resolved_breakpoints += resolved.len();
        self.emitted_pauses += pauses.len();

        let emitted = resolved.len() + pauses.len();
        for breakpoint in resolved {
            self.sink
                .emit(DebugAdapterEvent::BreakpointResolved(breakpoint));
        }
        for pause in pauses {
            self.sink.emit(DebugAdapterEvent::Paused(pause));
        }
        Ok(emitted)
    }

    fn attach(&mut self, vm: &mut Vm) -> Result<DebugAdapterResponse, VmError> {
        let session = DebugSession::new(vm.runtime())?;
        vm.attach_debug_session(session)?;
        let capabilities = self.capabilities;
        self.sink.emit(DebugAdapterEvent::Attached { capabilities });
        Ok(DebugAdapterResponse::Attached { capabilities })
    }

    fn set_breakpoint(
        &mut self,
        vm: &mut Vm,
        breakpoint: SourceBreakpoint,
    ) -> Result<DebugAdapterResponse, VmError> {
        let breakpoint_id = session_mut(vm)?.add_breakpoint(breakpoint)?;
        self.sink
            .emit(DebugAdapterEvent::BreakpointSet { breakpoint_id });
        Ok(DebugAdapterResponse::BreakpointSet { breakpoint_id })
    }
}

fn session(vm: &Vm) -> Result<&DebugSession, VmError> {
    vm.debug_session().ok_or(VmError::UnsupportedInstruction(
        "debug_session_not_attached",
    ))
}

fn session_mut(vm: &mut Vm) -> Result<&mut DebugSession, VmError> {
    vm.debug_session_mut()
        .ok_or(VmError::UnsupportedInstruction(
            "debug_session_not_attached",
        ))
}
