//! Frontend-free preparation of portable native input against validated bytecode.
use bincode::{DefaultOptions, Options};
use kagari_bytecode::{
    artifact::validate_program_resource_limits,
    program::{BytecodeProgram, verified::VerifiedBytecodeProgram},
};
use kagari_common::cancellation::CancellationToken;
use kagari_mir::{
    codec::{MirCodecError, decode_program},
    program::VerifiedMirProgram,
};

use crate::bytecode::{BytecodeLoweringError, lower_program_for_comparison};

#[derive(Debug, thiserror::Error)]
pub enum NativeInputError {
    #[error("native preparation cancelled")]
    Cancelled,
    #[error("invalid portable MIR: {0}")]
    Mir(MirCodecError),
    #[error("invalid native preparation bytecode: {0}")]
    Bytecode(String),
    #[error("MIR-to-bytecode lowering failed: {0:?}")]
    Lowering(BytecodeLoweringError),
    #[error("portable MIR does not reproduce the supplied bytecode program")]
    Mismatch,
}

impl From<MirCodecError> for NativeInputError {
    fn from(error: MirCodecError) -> Self {
        match error {
            MirCodecError::Cancelled => Self::Cancelled,
            error => Self::Mir(error),
        }
    }
}

/// Verify native input and establish complete canonical correspondence. Callers
/// must first validate their artifact envelope/manifest and do this before any
/// script effects. Hash equality between independent payloads is not sufficient.
/// The returned seal includes fresh linked contracts and program-point analyses.
pub fn verify_native_input(
    bytes: &[u8],
    bytecode: &VerifiedBytecodeProgram,
    cancel: &CancellationToken,
) -> Result<VerifiedMirProgram, NativeInputError> {
    check_cancel(cancel)?;
    let mir = decode_program(bytes, cancel)?;
    let lowered = lower_program_for_comparison(&mir).map_err(NativeInputError::Lowering)?;
    validate_program_resource_limits(&lowered)
        .map_err(|error| NativeInputError::Bytecode(error.to_string()))?;
    check_cancel(cancel)?;
    // Resource validation bounds both encodings before allocation. Comparing
    // canonical bytes preserves floating-point payloads and covers every field,
    // including layouts, concrete identities, imports, debug origins and budgets.
    let canonical = |program: &BytecodeProgram| {
        DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .serialize(program)
            .map_err(|error| NativeInputError::Bytecode(error.to_string()))
    };
    let expected = canonical(bytecode.program())?;
    check_cancel(cancel)?;
    let actual = canonical(&lowered)?;
    check_cancel(cancel)?;
    if actual != expected {
        return Err(NativeInputError::Mismatch);
    }
    // Exact canonical equality transfers the sealed bytecode's graph/bounds proof
    // to this lowering. The unsealed candidate is never returned or executed.
    Ok(mir)
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), NativeInputError> {
    cancel.check().map_err(|_| NativeInputError::Cancelled)
}
