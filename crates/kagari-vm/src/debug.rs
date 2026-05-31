use kagari_common::Span;
use kagari_ir::bytecode::{
    BytecodeFunction, BytecodeModule, DebugPointId, FunctionRef, LocalSlot, SafeDebugPoint,
};
use kagari_runtime::{ModuleId, value::Value};

use crate::{VmError, frame::Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DebugFrameId(u64);

impl DebugFrameId {
    pub fn index(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BreakpointId(u64);

impl BreakpointId {
    pub fn index(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBreakpoint {
    pub source_uri: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub source_offset: Option<usize>,
    pub temporary: bool,
}

impl SourceBreakpoint {
    pub fn at_source_offset(source_uri: impl Into<String>, source_offset: usize) -> Self {
        Self {
            source_uri: source_uri.into(),
            line: None,
            column: None,
            source_offset: Some(source_offset),
            temporary: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBreakpoint {
    pub breakpoint_id: BreakpointId,
    pub module_id: ModuleId,
    pub epoch: u64,
    pub function: FunctionRef,
    pub instruction_offset: usize,
    pub source_span: Span,
    pub debug_point: DebugPointId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugPauseReason {
    ManualPause,
    Breakpoint(BreakpointId),
    Step,
    Trap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DebugPause {
    pub reason: DebugPauseReason,
    pub frames: Vec<DebugFrame>,
}

impl DebugPause {
    pub fn top_frame(&self) -> Option<&DebugFrame> {
        self.frames.last()
    }

    pub fn evaluate_watch(
        &self,
        frame_id: DebugFrameId,
        watch: &DebugWatch,
    ) -> Result<Value, VmError> {
        let frame = self
            .frames
            .iter()
            .find(|frame| frame.id == frame_id)
            .ok_or(VmError::UnsupportedInstruction("debug_watch_frame"))?;
        match watch {
            DebugWatch::Binding(name) => frame
                .bindings
                .iter()
                .find(|binding| binding.name == *name)
                .map(|binding| binding.value.clone())
                .ok_or(VmError::MissingField(name.clone())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DebugFrame {
    pub id: DebugFrameId,
    pub module_id: ModuleId,
    pub epoch: u64,
    pub function: FunctionRef,
    pub function_name: String,
    pub instruction_offset: usize,
    pub source_span: Span,
    pub bindings: Vec<DebugBinding>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DebugBinding {
    pub name: String,
    pub local: LocalSlot,
    pub value: Value,
    pub is_parameter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugWatch {
    Binding(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepMode {
    Continue,
    PauseNext,
    StepOver { depth: usize },
    StepOut { depth: usize },
}

#[derive(Debug, Clone)]
struct RegisteredBreakpoint {
    breakpoint: SourceBreakpoint,
    id: BreakpointId,
}

#[derive(Debug, Clone)]
pub struct DebugSession {
    breakpoints: Vec<RegisteredBreakpoint>,
    resolved: Vec<ResolvedBreakpoint>,
    pauses: Vec<DebugPause>,
    next_breakpoint_id: u64,
    next_frame_id: u64,
    mode: StepMode,
}

impl Default for DebugSession {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugSession {
    pub fn new() -> Self {
        Self {
            breakpoints: Vec::new(),
            resolved: Vec::new(),
            pauses: Vec::new(),
            next_breakpoint_id: 0,
            next_frame_id: 0,
            mode: StepMode::Continue,
        }
    }

    pub fn add_breakpoint(&mut self, breakpoint: SourceBreakpoint) -> BreakpointId {
        let id = BreakpointId(self.next_breakpoint_id);
        self.next_breakpoint_id += 1;
        self.breakpoints
            .push(RegisteredBreakpoint { breakpoint, id });
        id
    }

    pub fn resolved_breakpoints(&self) -> &[ResolvedBreakpoint] {
        &self.resolved
    }

    pub fn pauses(&self) -> &[DebugPause] {
        &self.pauses
    }

    pub fn pause(&mut self) {
        self.mode = StepMode::PauseNext;
    }

    pub fn continue_execution(&mut self) {
        self.mode = StepMode::Continue;
    }

    pub fn step_into(&mut self) {
        self.mode = StepMode::PauseNext;
    }

    pub fn step_over(&mut self, current_depth: usize) {
        self.mode = StepMode::StepOver {
            depth: current_depth,
        };
    }

    pub fn step_out(&mut self, current_depth: usize) {
        self.mode = StepMode::StepOut {
            depth: current_depth,
        };
    }

    pub fn run_to_cursor(
        &mut self,
        source_uri: impl Into<String>,
        source_offset: usize,
    ) -> BreakpointId {
        let mut breakpoint = SourceBreakpoint::at_source_offset(source_uri, source_offset);
        breakpoint.temporary = true;
        self.continue_execution();
        self.add_breakpoint(breakpoint)
    }

    pub(crate) fn resolve_module(
        &mut self,
        module_id: ModuleId,
        module_name: &str,
        epoch: u64,
        module: &BytecodeModule,
    ) {
        self.resolved
            .retain(|resolved| resolved.module_id != module_id || resolved.epoch != epoch);
        for function in &module.functions {
            for point in &function.metadata.debug.safe_debug_points {
                for breakpoint in &self.breakpoints {
                    if breakpoint.breakpoint.source_uri == module_name
                        && breakpoint_matches(function, &breakpoint.breakpoint, point)
                    {
                        self.resolved.push(ResolvedBreakpoint {
                            breakpoint_id: breakpoint.id,
                            module_id,
                            epoch,
                            function: function.id,
                            instruction_offset: point.instruction_offset,
                            source_span: point.span,
                            debug_point: point.id,
                        });
                    }
                }
            }
        }
    }

    pub(crate) fn before_instruction(
        &mut self,
        module_id: ModuleId,
        epoch: u64,
        frames: &[Frame<'_>],
    ) -> Result<(), VmError> {
        let Some(frame) = frames.last() else {
            return Ok(());
        };
        let offset = frame.instruction_offset();
        let Some(_point) = safe_debug_point(frame.function(), offset) else {
            return Ok(());
        };

        let breakpoint = self
            .resolved
            .iter()
            .find(|resolved| {
                resolved.module_id == module_id
                    && resolved.epoch == epoch
                    && resolved.function == frame.function().id
                    && resolved.instruction_offset == offset
            })
            .map(|resolved| resolved.breakpoint_id);
        let reason = match breakpoint {
            Some(id) => Some(DebugPauseReason::Breakpoint(id)),
            None => self.step_reason(frames.len()),
        };

        if let Some(reason) = reason {
            self.record_pause(reason, module_id, epoch, frames)?;
        }
        if let Some(id) = breakpoint {
            let temporary = self
                .breakpoints
                .iter()
                .any(|breakpoint| breakpoint.id == id && breakpoint.breakpoint.temporary);
            self.breakpoints
                .retain(|breakpoint| breakpoint.id != id || !breakpoint.breakpoint.temporary);
            if temporary {
                self.resolved
                    .retain(|resolved| resolved.breakpoint_id != id);
            }
        }
        Ok(())
    }

    pub(crate) fn record_trap(
        &mut self,
        module_id: ModuleId,
        epoch: u64,
        frames: &[Frame<'_>],
    ) -> Result<(), VmError> {
        self.record_pause(DebugPauseReason::Trap, module_id, epoch, frames)
    }

    fn step_reason(&mut self, depth: usize) -> Option<DebugPauseReason> {
        match self.mode {
            StepMode::Continue => None,
            StepMode::PauseNext => {
                self.mode = StepMode::Continue;
                Some(DebugPauseReason::Step)
            }
            StepMode::StepOver { depth: target } if depth <= target => {
                self.mode = StepMode::Continue;
                Some(DebugPauseReason::Step)
            }
            StepMode::StepOut { depth: target } if depth < target => {
                self.mode = StepMode::Continue;
                Some(DebugPauseReason::Step)
            }
            StepMode::StepOver { .. } | StepMode::StepOut { .. } => None,
        }
    }

    fn record_pause(
        &mut self,
        reason: DebugPauseReason,
        module_id: ModuleId,
        epoch: u64,
        frames: &[Frame<'_>],
    ) -> Result<(), VmError> {
        let pause = DebugPause {
            reason,
            frames: frames
                .iter()
                .map(|frame| self.inspect_frame(module_id, epoch, frame))
                .collect::<Result<Vec<_>, _>>()?,
        };
        self.pauses.push(pause);
        Ok(())
    }

    fn inspect_frame(
        &mut self,
        module_id: ModuleId,
        epoch: u64,
        frame: &Frame<'_>,
    ) -> Result<DebugFrame, VmError> {
        let id = DebugFrameId(self.next_frame_id);
        self.next_frame_id += 1;
        let instruction_offset = frame.instruction_offset();
        let source_span = source_span_for(frame.function(), instruction_offset);
        let bindings = frame
            .function()
            .metadata
            .debug
            .local_live_ranges
            .iter()
            .filter(|range| range.start <= instruction_offset && instruction_offset <= range.end)
            .map(|range| {
                Ok(DebugBinding {
                    name: range.name.clone(),
                    local: range.local,
                    value: frame.read_local(range.local)?,
                    is_parameter: range.is_parameter,
                })
            })
            .collect::<Result<Vec<_>, VmError>>()?;

        Ok(DebugFrame {
            id,
            module_id,
            epoch,
            function: frame.function().id,
            function_name: frame.function().name.clone(),
            instruction_offset,
            source_span,
            bindings,
        })
    }
}

fn breakpoint_matches(
    function: &BytecodeFunction,
    breakpoint: &SourceBreakpoint,
    point: &SafeDebugPoint,
) -> bool {
    if let Some(offset) = breakpoint.source_offset
        && point.span.start <= offset
        && offset <= point.span.end
    {
        return true;
    }
    if let Some(line) = breakpoint.line {
        return function.metadata.debug.line_table.iter().any(|entry| {
            entry.instruction_offset == point.instruction_offset
                && entry.line == Some(line)
                && breakpoint
                    .column
                    .is_none_or(|column| entry.column == Some(column))
        });
    }
    false
}

fn safe_debug_point(
    function: &BytecodeFunction,
    instruction_offset: usize,
) -> Option<&SafeDebugPoint> {
    function
        .metadata
        .debug
        .safe_debug_points
        .iter()
        .find(|point| point.instruction_offset == instruction_offset)
}

fn source_span_for(function: &BytecodeFunction, instruction_offset: usize) -> Span {
    function
        .metadata
        .debug
        .source_spans
        .iter()
        .find(|span| span.instruction_offset == instruction_offset)
        .map(|span| span.span)
        .unwrap_or_default()
}
