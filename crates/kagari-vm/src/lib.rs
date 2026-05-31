mod debug;
mod debug_protocol;
mod error;
mod executor;
mod frame;
mod vm;

pub use debug::{
    BreakpointId, DebugBinding, DebugFrame, DebugFrameId, DebugPause, DebugPauseReason,
    DebugSession, DebugWatch, ResolvedBreakpoint, SourceBreakpoint,
};
pub use debug_protocol::{
    DebugAdapterCapabilities, DebugAdapterEvent, DebugAdapterEventSink, DebugAdapterRequest,
    DebugAdapterResponse, DebugProtocolAdapter, RecordingDebugAdapterSink,
};
pub use error::VmError;
pub use vm::{ExecutionReport, JitExecutionReport, JitExecutionStatus, Vm};

#[cfg(test)]
mod tests;
