mod debug;
mod error;
mod executor;
mod frame;
mod vm;

pub use debug::{
    BreakpointId, DebugBinding, DebugFrame, DebugFrameId, DebugPause, DebugPauseReason,
    DebugSession, DebugWatch, ResolvedBreakpoint, SourceBreakpoint,
};
pub use error::VmError;
pub use vm::{ExecutionReport, Vm};

#[cfg(test)]
mod tests;
