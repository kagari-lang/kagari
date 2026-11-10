use crate::error::RuntimeError;
use kagari_bytecode::module::BytecodeFunction;
use kagari_contract::native::ExecutableDebugInfo;
pub mod native;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendInvocationError {
    #[error("{0}")]
    UnsupportedArtifact(String),
    #[error(transparent)]
    RuntimeFailure(RuntimeError),
    #[error("{0}")]
    InternalError(String),
}

pub fn missing_debug_requirements(
    debug: &ExecutableDebugInfo,
    function: &BytecodeFunction,
) -> Vec<String> {
    let mut missing = Vec::new();
    if !debug.has_line_tables {
        missing.push("line tables".to_owned());
    }
    if !debug.has_source_spans {
        missing.push("source span mapping".to_owned());
    }
    if !debug.has_live_value_locations {
        missing.push("live value locations".to_owned());
    }
    if !debug.has_safe_debug_callbacks {
        missing.push("safe debug point callbacks".to_owned());
    }
    for point in &function.metadata.debug.safe_debug_points {
        if !debug.safe_debug_points.iter().any(|candidate| {
            candidate.instruction_offset == point.instruction_offset
                && candidate.debug_point == point.id
        }) {
            missing.push(format!(
                "safe debug point {} at instruction {}",
                point.id.index(),
                point.instruction_offset
            ));
        }
    }
    missing
}
