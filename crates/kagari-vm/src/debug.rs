use kagari_common::Span;
use kagari_ir::bytecode::{
    BytecodeFunction, BytecodeModule, DebugPointId, FunctionRef, LocalSlot, SafeDebugPoint,
};
use kagari_runtime::{
    DebugVisibilityPolicy, ModuleId, Runtime, RuntimeError, SecurityContext, value::Value,
};

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
        runtime: &Runtime,
        frame_id: DebugFrameId,
        watch: &DebugWatch,
    ) -> Result<Value, VmError> {
        runtime
            .validate_debug_watch_evaluation_boundary()
            .map_err(VmError::RuntimeError)?;
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
                .map(|binding| {
                    runtime
                        .validate_debug_value_visible(&binding.value)
                        .map_err(VmError::RuntimeError)?;
                    Ok(binding.value.clone())
                })
                .transpose()?
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
    security: SecurityContext,
    visibility: DebugVisibilityPolicy,
    breakpoints: Vec<RegisteredBreakpoint>,
    resolved: Vec<ResolvedBreakpoint>,
    pauses: Vec<DebugPause>,
    next_breakpoint_id: u64,
    next_frame_id: u64,
    mode: StepMode,
}

impl DebugSession {
    pub fn new(runtime: &Runtime) -> Result<Self, VmError> {
        runtime
            .validate_debug_attach_boundary()
            .map_err(VmError::RuntimeError)?;
        Ok(Self {
            security: runtime.security(),
            visibility: runtime.debug_visibility().clone(),
            breakpoints: Vec::new(),
            resolved: Vec::new(),
            pauses: Vec::new(),
            next_breakpoint_id: 0,
            next_frame_id: 0,
            mode: StepMode::Continue,
        })
    }

    pub fn add_breakpoint(
        &mut self,
        breakpoint: SourceBreakpoint,
    ) -> Result<BreakpointId, VmError> {
        self.validate_breakpoints()?;
        self.validate_visible_module(&breakpoint.source_uri)?;
        let id = BreakpointId(self.next_breakpoint_id);
        self.next_breakpoint_id += 1;
        self.breakpoints
            .push(RegisteredBreakpoint { breakpoint, id });
        Ok(id)
    }

    pub fn resolved_breakpoints(&self) -> &[ResolvedBreakpoint] {
        &self.resolved
    }

    pub fn pauses(&self) -> &[DebugPause] {
        &self.pauses
    }

    pub fn pause(&mut self) -> Result<(), VmError> {
        self.validate_pause()?;
        self.mode = StepMode::PauseNext;
        Ok(())
    }

    pub fn continue_execution(&mut self) -> Result<(), VmError> {
        self.validate_pause()?;
        self.mode = StepMode::Continue;
        Ok(())
    }

    pub fn step_into(&mut self) -> Result<(), VmError> {
        self.validate_pause()?;
        self.mode = StepMode::PauseNext;
        Ok(())
    }

    pub fn step_over(&mut self, current_depth: usize) -> Result<(), VmError> {
        self.validate_pause()?;
        self.mode = StepMode::StepOver {
            depth: current_depth,
        };
        Ok(())
    }

    pub fn step_out(&mut self, current_depth: usize) -> Result<(), VmError> {
        self.validate_pause()?;
        self.mode = StepMode::StepOut {
            depth: current_depth,
        };
        Ok(())
    }

    pub fn run_to_cursor(
        &mut self,
        source_uri: impl Into<String>,
        source_offset: usize,
    ) -> Result<BreakpointId, VmError> {
        let mut breakpoint = SourceBreakpoint::at_source_offset(source_uri, source_offset);
        breakpoint.temporary = true;
        self.continue_execution()?;
        self.add_breakpoint(breakpoint)
    }

    pub(crate) fn resolve_module(
        &mut self,
        module_id: ModuleId,
        module_name: &str,
        epoch: u64,
        module: &BytecodeModule,
        runtime: &Runtime,
    ) -> Result<(), VmError> {
        self.resolved
            .retain(|resolved| resolved.module_id != module_id || resolved.epoch != epoch);
        if self.breakpoints.is_empty() {
            return Ok(());
        }
        if runtime.validate_debug_module_visible(module_name).is_err() {
            return Ok(());
        }
        runtime
            .validate_debug_breakpoint_boundary()
            .map_err(VmError::RuntimeError)?;
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
        Ok(())
    }

    pub(crate) fn before_instruction(
        &mut self,
        runtime: &Runtime,
        module_name: &str,
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
            Some(id) => {
                runtime
                    .validate_debug_breakpoint_boundary()
                    .map_err(VmError::RuntimeError)?;
                Some(DebugPauseReason::Breakpoint(id))
            }
            None => self.step_reason(frames.len()),
        };

        if let Some(reason) = reason {
            runtime
                .validate_debug_module_visible(module_name)
                .map_err(VmError::RuntimeError)?;
            self.record_pause(runtime, reason, module_id, epoch, frames)?;
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
        runtime: &Runtime,
        module_id: ModuleId,
        epoch: u64,
        frames: &[Frame<'_>],
    ) -> Result<(), VmError> {
        runtime
            .validate_debug_pause_boundary()
            .map_err(VmError::RuntimeError)?;
        self.record_pause(runtime, DebugPauseReason::Trap, module_id, epoch, frames)
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
        runtime: &Runtime,
        reason: DebugPauseReason,
        module_id: ModuleId,
        epoch: u64,
        frames: &[Frame<'_>],
    ) -> Result<(), VmError> {
        runtime
            .validate_debug_stack_inspection_boundary()
            .map_err(VmError::RuntimeError)?;
        let pause = DebugPause {
            reason,
            frames: frames
                .iter()
                .map(|frame| self.inspect_frame(runtime, module_id, epoch, frame))
                .collect::<Result<Vec<_>, _>>()?,
        };
        self.pauses.push(pause);
        Ok(())
    }

    fn inspect_frame(
        &mut self,
        runtime: &Runtime,
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
                let value = frame.read_local(range.local)?;
                runtime
                    .validate_debug_value_visible(&value)
                    .map_err(VmError::RuntimeError)?;
                Ok(DebugBinding {
                    name: range.name.clone(),
                    local: range.local,
                    value,
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

    fn validate_breakpoints(&self) -> Result<(), VmError> {
        if self.security.allows_debug_breakpoints() {
            Ok(())
        } else {
            Err(VmError::RuntimeError(RuntimeError::capability_denied(
                "debug_breakpoints",
            )))
        }
    }

    fn validate_pause(&self) -> Result<(), VmError> {
        if self.security.allows_debug_pause() {
            Ok(())
        } else {
            Err(VmError::RuntimeError(RuntimeError::capability_denied(
                "debug_pause",
            )))
        }
    }

    fn validate_visible_module(&self, module_name: &str) -> Result<(), VmError> {
        if self.visibility.exposes_module(module_name) {
            Ok(())
        } else {
            Err(VmError::RuntimeError(RuntimeError::capability_denied(
                format!("debug module `{module_name}`"),
            )))
        }
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
