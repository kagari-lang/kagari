pub use kagari_hir::builtin::{BuiltinMethod, surface::StandardIntrinsic};

mod artifact;
mod instruction;
mod lower;
mod module;
mod verifier;

pub use artifact::{
    ArtifactBuildOptions, ArtifactCodecError, ArtifactCompatibility, ArtifactEncoding,
    ArtifactFingerprint, ArtifactHeader, ArtifactModuleIdentity, ArtifactSection,
    ArtifactSectionBuffer, ArtifactSectionId, ArtifactSignature, ArtifactSignatures,
    ArtifactTables, ArtifactValidationError, ControlFlowTargetMetadata,
    ControlFlowTargetMetadataBuffer, DebugMetadata, DebugNameTable, DependencyFingerprint,
    DependencyFingerprintBuffer, FunctionEffectBuffer, FunctionEffectMetadata,
    FunctionLayoutBuffer, FunctionLayoutMetadata, HostDependencyTable, KAGARI_COMPILER_FINGERPRINT,
    KAGARI_LANGUAGE_VERSION, KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION,
    KBC_ARTIFACT_FORMAT_VERSION, KBC_MAGIC, KbcArtifact, LoaderValidationMetadata, ModuleEpoch,
    PathDescriptorFingerprint, PathFingerprintBuffer, PublicAbiFingerprint,
    PublicAbiFingerprintBuffer, SourceFileTable, VerificationMetadata,
};
pub use instruction::{
    BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, FieldId, FunctionRef, JumpTarget,
    LocalSlot, ModuleSlot, PathId, Register, RuntimeHelper, StructFieldInit, UnaryOp,
};
pub use lower::{BytecodeLoweringError, lower_to_bytecode};
pub use module::{
    BytecodeDebugMetadata, BytecodeFunction, BytecodeFunctionBuffer, BytecodeInstructionBuffer,
    BytecodeModule, BytecodeModuleSlot, BytecodeModuleSlotBuffer, BytecodeTypeTable,
    CapturedBindingDebugBuffer, CapturedBindingDebugInfo, ConstantPool, ControlFlowTargetBuffer,
    DebugPointId, FieldRecord, FieldTable, FrameLayout, FunctionMetadata, FunctionRecord,
    FunctionTable, InstructionSourceSpan, InstructionSourceSpanBuffer, LineTableBuffer,
    LineTableEntry, LocalLiveRange, LocalLiveRangeBuffer, PathRecord, PathTable, PublicItemRecord,
    PublicItemTable, SafeDebugPoint, SafeDebugPointBuffer, SafeDebugPointKind, TypeLayoutBuffer,
};
pub use verifier::{BytecodeVerificationError, verify_module};
