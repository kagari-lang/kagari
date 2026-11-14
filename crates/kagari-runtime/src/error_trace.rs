//! Diagnostic snapshots contain no script values, roots or execution-version handles.
pub mod asynchronous;

use crate::{
    Runtime,
    error::RuntimeError,
    error_trace::asynchronous::AsyncBoundary,
    execution::roots::SessionId,
    frame::ExecutionFrame,
    module::ExecutableModule,
    native::conversion::context::ConversionContext,
    resource::ResourceState,
    session::native::NativeFrame,
    value::{aggregates::enumerations::EnumObject, carriers::RootedValue, kind::ObjectKind},
    value_semantics::formatting,
};
use kagari_bytecode::{artifact::ArtifactFingerprint, module::CallableTarget, program::ModuleRef};
use kagari_common::span::Span;
use std::{
    fmt,
    fmt::{Display, Formatter},
    iter,
    sync::Arc,
};

pub const MAX_ERROR_FRAMES: usize = 128;
const MAX_LABEL_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorFrame {
    pub epoch: u64,
    pub code_fingerprint: ArtifactFingerprint,
    /// Module slot in the fingerprinted program; target indices are module-local.
    pub module: ModuleRef,
    pub target: CallableTarget,
    pub function_name: String,
    pub source_uri: String,
    pub instruction_offset: usize,
    /// At least one earlier scalar self-tail caller was replaced in this chain.
    pub tail_calls_elided: bool,
    pub semantic_origin: Option<usize>,
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
    /// Detached causal boundaries, from the original failure toward its observers.
    pub async_boundaries: Vec<AsyncBoundary>,
    pub omitted_async_boundaries: usize,
}

enum TraceSource<'frame> {
    Physical(&'frame ExecutionFrame),
    Native(&'frame NativeFrame),
}

impl ErrorTrace {
    pub(crate) fn capture(resources: &ResourceState) -> Arc<Self> {
        let Some(session) = resources.active_session() else {
            return Arc::new(Self {
                incomplete: true,
                ..Self::default()
            });
        };
        Self::capture_session(resources, session.id)
    }

    pub(crate) fn capture_session(resources: &ResourceState, id: SessionId) -> Arc<Self> {
        let Some(frames) = resources.sessions.frames(id) else {
            return Arc::new(Self {
                incomplete: true,
                ..Self::default()
            });
        };
        let Some(state) = resources.sessions.get(id) else {
            return Self::from_frames(&frames);
        };
        let native = state.native_calls.borrow();
        Self::from_parts(&frames, &native)
    }

    fn from_frames(frames: &[ExecutionFrame]) -> Arc<Self> {
        Self::from_parts(frames, &[])
    }

    fn from_parts(frames: &[ExecutionFrame], native: &[NativeFrame]) -> Arc<Self> {
        let total = frames.len().saturating_add(native.len());
        let count = total.min(MAX_ERROR_FRAMES);
        let mut trace = Self {
            omitted_frames: total - count,
            incomplete: total == 0,
            ..Self::default()
        };
        if trace.frames.try_reserve(count).is_err() {
            trace.omitted_frames = total;
            trace.incomplete = true;
            return Arc::new(trace);
        }
        let mut physical = frames.iter().enumerate().rev().peekable();
        let mut native = native.iter().rev().peekable();
        let ordered = iter::from_fn(|| {
            if let Some(frame) = native.peek()
                && physical
                    .peek()
                    .is_none_or(|(index, _)| frame.depth > *index)
            {
                return native.next().map(TraceSource::Native);
            }
            physical
                .next()
                .map(|(_, frame)| TraceSource::Physical(frame))
        });
        for source in ordered.take(count) {
            let (loaded, target, function, offset, native_origin, tail_calls_elided) = match source
            {
                TraceSource::Physical(frame) => (
                    frame.loaded(),
                    frame.target(),
                    frame.function(),
                    frame.instruction_offset(),
                    frame.native_origin(),
                    frame.tail_calls_elided(),
                ),
                TraceSource::Native(frame) => (
                    &frame.owner,
                    CallableTarget::Native(frame.import),
                    None,
                    0,
                    None,
                    false,
                ),
            };
            let module = loaded.slot();
            let Some(function) = function else {
                let CallableTarget::Native(import) = target else {
                    unreachable!("native frame target");
                };
                let contract = &loaded.bytecode.native_imports[import.index()];
                let name = loaded
                    .definition_name(contract.instance.declaration)
                    .unwrap_or("<native>");
                let Some(function_name) = label(name, &mut trace.incomplete) else {
                    break;
                };
                let Some(source_uri) = label(&loaded.name, &mut trace.incomplete) else {
                    break;
                };
                trace.frames.push(ErrorFrame {
                    epoch: loaded.epoch.0,
                    code_fingerprint: loaded.program_fingerprint(),
                    module,
                    target,
                    function_name,
                    source_uri,
                    instruction_offset: 0,
                    tail_calls_elided: false,
                    semantic_origin: None,
                    source_span: None,
                    line: None,
                    column: None,
                });
                continue;
            };
            let debug = &function.metadata.debug;
            let semantic_origin = native_origin.or_else(|| {
                function
                    .metadata
                    .instruction_origin(offset)
                    .map(|(id, _)| id)
            });
            let point = semantic_origin.and_then(|id| function.metadata.origins.get(id));
            let origin = debug.source_module.and_then(|slot| loaded.member(slot));
            let loaded = origin.as_ref().unwrap_or(loaded);
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
                module,
                target,
                function_name,
                source_uri,
                instruction_offset: offset,
                tail_calls_elided,
                semantic_origin,
                source_span: point.map(|origin| origin.span),
                line: point.and_then(|origin| origin.line),
                column: point.and_then(|origin| origin.column),
            });
        }
        trace.omitted_frames = total - trace.frames.len();
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

impl Display for ErrorTrace {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for frame in &self.frames {
            write!(f, "\n  at {} ({}", frame.function_name, frame.source_uri)?;
            if let (Some(line), Some(column)) = (frame.line, frame.column) {
                write!(f, ":{line}:{column}")?;
            } else if let Some(span) = frame.source_span {
                write!(f, ":byte {}", span.start)?;
            }
            write!(f, "; epoch {})", frame.epoch)?;
            if frame.tail_calls_elided {
                write!(f, " [self-tail callers elided]")?;
            }
        }
        if self.omitted_frames > 0 {
            write!(f, "\n  ... {} frames omitted", self.omitted_frames)?;
        }
        for boundary in &self.async_boundaries {
            write!(f, "\n  {boundary}")?;
        }
        if self.omitted_async_boundaries > 0 {
            write!(
                f,
                "\n  ... {} async boundaries omitted",
                self.omitted_async_boundaries
            )?;
        }
        if self.incomplete {
            write!(f, "\n  [error trace incomplete or unavailable]")?;
        }
        Ok(())
    }
}

impl Runtime {
    pub fn capture_error_trace(&self) -> Arc<ErrorTrace> {
        ErrorTrace::capture(self.resources())
    }
}

/// A detached diagnostic preview, not a script value and not an execution failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultFailure {
    pub message: String,
    pub trace: Arc<ErrorTrace>,
}

impl Display for ResultFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.message, self.trace)
    }
}

impl Runtime {
    pub fn result_failure(&mut self, value: &RootedValue) -> Option<ResultFailure> {
        self.memory
            .scope(&self.resources, |cx, _| {
                let value = value.get(cx)?;
                if value.kind() != Some(ObjectKind::Enum) {
                    return Ok(None);
                }
                let object = cx.downcast::<EnumObject>(value.object().expect("managed enum"))?;
                let trace = cx.read(object, |object| {
                    object
                        .layout()
                        .variant()
                        .reports_failure
                        .then(|| object.origin().cloned())
                        .flatten()
                });
                let Some(trace) = trace else {
                    return Ok(None);
                };
                if cx.read(object, EnumObject::len) == 0 {
                    return Ok(None);
                }
                let payload = EnumObject::get(cx, object, 0)?;
                // A detached business-error preview is bounded data inspection. It
                // cannot terminate completed execution or invoke user formatting.
                let preview = formatting::format_value(
                    cx,
                    &mut || Ok(()),
                    payload,
                    payload.kind() != Some(ObjectKind::String),
                )
                .unwrap_or_else(|_| "<error payload unavailable>".into());
                let mut clipped = false;
                let message = label(&preview, &mut clipped)
                    .unwrap_or_else(|| "<error payload unavailable>".into());
                let message = if clipped {
                    format!("{message} [truncated]")
                } else {
                    message
                };
                Ok(Some(ResultFailure { message, trace }))
            })
            .ok()
            .flatten()
    }

    pub fn forward_enum_origin(
        &mut self,
        owner: &ExecutableModule,
        original: &RootedValue,
        value: &RootedValue,
    ) -> Result<RootedValue, RuntimeError> {
        self.validate_executable_module(owner)?;
        let mut conversion = ConversionContext::new(self, owner)?;
        let original = conversion.import(original)?;
        let value = conversion.import(value)?;
        let value = conversion.forward_enum_origin(&original, &value)?;
        conversion.retain(&value)
    }
}
