//! Diagnostic snapshots contain no script values, roots or execution-version handles.
use crate::{Runtime, frame::ExecutionFrame};
use kagari_common::Span;
use kagari_ir::bytecode::{ArtifactFingerprint, FunctionRef};
use std::sync::Arc;

pub const MAX_ERROR_FRAMES: usize = 128;
const MAX_LABEL_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorFrame {
    pub epoch: u64,
    pub code_fingerprint: ArtifactFingerprint,
    pub function: FunctionRef,
    pub function_name: String,
    pub source_uri: String,
    pub instruction_offset: usize,
    pub source_span: Option<Span>,
    /// One-based line and UTF-8 byte column, independent of the current disk source.
    pub line: Option<u32>,
    pub column: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ErrorTrace {
    /// Innermost frame first: the first frame is the failure's origin.
    pub frames: Vec<ErrorFrame>,
    pub omitted_frames: usize,
    pub incomplete: bool,
}
impl ErrorTrace {
    pub(crate) fn capture(resources: &crate::ResourceState) -> Arc<Self> {
        let Some(session) = resources.active_session() else {
            return Arc::new(Self {
                incomplete: true,
                ..Self::default()
            });
        };
        Self::capture_session(&session)
    }
    pub(crate) fn capture_session(session: &crate::session::SessionState) -> Arc<Self> {
        let Ok(frames) = session.frames.try_borrow() else {
            return Arc::new(Self {
                incomplete: true,
                ..Self::default()
            });
        };
        Self::from_frames(&frames)
    }
    fn from_frames(frames: &[ExecutionFrame]) -> Arc<Self> {
        let count = frames.len().min(MAX_ERROR_FRAMES);
        let mut trace = Self {
            omitted_frames: frames.len() - count,
            incomplete: frames.is_empty(),
            ..Self::default()
        };
        if trace.frames.try_reserve(count).is_err() {
            trace.omitted_frames = frames.len();
            trace.incomplete = true;
            return Arc::new(trace);
        }
        for frame in frames.iter().rev().take(count) {
            let function = frame.function();
            let offset = frame.instruction_offset();
            let debug = &function.metadata.debug;
            let line = debug
                .line_table
                .iter()
                .find(|entry| entry.instruction_offset == offset);
            let source_span = debug
                .source_spans
                .iter()
                .find(|entry| entry.instruction_offset == offset)
                .map(|entry| entry.span);
            let origin = debug
                .source_module
                .and_then(|slot| frame.loaded().member(slot));
            let loaded = origin.as_ref().unwrap_or(frame.loaded());
            let Some(function_name) = label(&function.name, &mut trace.incomplete) else {
                trace.incomplete = true;
                break;
            };
            let Some(source_uri) = label(
                debug.source_uri.as_deref().unwrap_or(&loaded.name),
                &mut trace.incomplete,
            ) else {
                trace.incomplete = true;
                break;
            };
            trace.frames.push(ErrorFrame {
                epoch: loaded.epoch.0,
                code_fingerprint: loaded.program_fingerprint(),
                function: function.id,
                function_name,
                source_uri,
                instruction_offset: offset,
                source_span,
                line: line.and_then(|l| l.line),
                column: line.and_then(|l| l.column),
            });
        }
        trace.omitted_frames = frames.len() - trace.frames.len();
        Arc::new(trace)
    }
}
fn label(value: &str, incomplete: &mut bool) -> Option<String> {
    let mut end = value.len().min(MAX_LABEL_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    *incomplete |= end < value.len();
    let mut result = String::new();
    result.try_reserve(end).ok()?;
    result.push_str(&value[..end]);
    Some(result)
}
impl std::fmt::Display for ErrorTrace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for frame in &self.frames {
            write!(f, "\n  at {} ({}", frame.function_name, frame.source_uri)?;
            if let (Some(line), Some(column)) = (frame.line, frame.column) {
                write!(f, ":{line}:{column}")?;
            } else if let Some(span) = frame.source_span {
                write!(f, ":byte {}", span.start)?;
            }
            write!(f, "; epoch {})", frame.epoch)?;
        }
        if self.omitted_frames > 0 {
            write!(f, "\n  ... {} frames omitted", self.omitted_frames)?;
        }
        if self.incomplete {
            write!(f, "\n  [error trace incomplete or unavailable]")?;
        }
        Ok(())
    }
}
impl Runtime {
    pub fn capture_error_trace(&self) -> Arc<ErrorTrace> {
        ErrorTrace::capture(&self.resources)
    }
    /// Native backends publish their logical program point before a budget check.
    pub fn record_native_instruction(&self, offset: usize) -> Result<(), crate::RuntimeError> {
        if let Some(session) = self.resources.active_session() {
            let mut frames = session.frames.try_borrow_mut().map_err(|_| {
                self.resources
                    .quarantine("native program point while frames are borrowed")
            })?;
            if let Some(frame) = frames.last_mut() {
                frame.set_native_instruction(offset)?;
            }
        }
        Ok(())
    }
}

/// A detached diagnostic preview, not a script value and not an execution failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultFailure {
    pub message: String,
    pub trace: Arc<ErrorTrace>,
}
impl std::fmt::Display for ResultFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.message, self.trace)
    }
}
impl Runtime {
    pub fn result_failure(&self, value: &crate::value::Value) -> Option<ResultFailure> {
        let trace = self.gc.result_error_trace(value)?;
        let crate::value::Value::Enum(id) = value else {
            return None;
        };
        let snapshot = self.gc.enum_snapshot(*id)?;
        let payload = snapshot.fields.first()?;
        let preview = crate::value_semantics::format_value(
            &self.gc,
            payload,
            !matches!(payload, crate::value::Value::Str(_)),
        )
        .unwrap_or_else(|_| "<error payload unavailable>".into());
        let mut clipped = false;
        let message =
            label(&preview, &mut clipped).unwrap_or_else(|| "<error payload unavailable>".into());
        let message = if clipped {
            format!("{message} [truncated]")
        } else {
            message
        };
        Some(ResultFailure { message, trace })
    }
    pub fn map_result_error(
        &self,
        owner: &crate::LoadedModule,
        original: &crate::value::Value,
        error: crate::value::Value,
        ty: &kagari_ir::module::abi::AbiType,
    ) -> Result<crate::value::Value, crate::RuntimeError> {
        use kagari_ir::module::abi::{AbiType, StandardEnumKind};
        self.validate_loaded_module(owner)?;
        if let AbiType::StandardEnum {
            kind: StandardEnumKind::Result,
            args,
        } = ty
            && args.len() == 2
            && self.gc.matches_abi(&error, &args[1], owner)
        {
            return self
                .gc
                .map_result_error(original, error)
                .map(crate::value::Value::Enum);
        }
        Err(crate::RuntimeError::new(
            crate::RuntimeErrorKind::ScriptTrap,
            "invalid mapped Result error type",
        ))
    }
}
