pub use kagari_hir::builtin::surface::StandardIntrinsic;

mod artifact;
mod instruction;
mod lower;
mod module;
mod program;
mod verifier;

pub use artifact::{
    ArtifactBuildOptions, ArtifactCodecError, ArtifactCompatibility, ArtifactEncoding,
    ArtifactFingerprint, ArtifactHeader, ArtifactSection, ArtifactSectionBuffer, ArtifactSectionId,
    ArtifactSignature, ArtifactSignatures, ArtifactTables, ArtifactValidationError,
    ControlFlowTargetMetadata, ControlFlowTargetMetadataBuffer, DebugMetadata, DebugNameTable,
    DependencyFingerprint, DependencyFingerprintBuffer, FunctionEffectBuffer,
    FunctionEffectMetadata, FunctionLayoutBuffer, FunctionLayoutMetadata,
    KAGARI_COMPILER_FINGERPRINT, KAGARI_LANGUAGE_VERSION, KAGARI_RUNTIME_ABI_VERSION,
    KAGARI_RUNTIME_HELPER_ABI_VERSION, KBC_ARTIFACT_FORMAT_VERSION, KBC_MAGIC, KbcArtifact,
    LoaderValidationMetadata, ModuleEpoch, PathDescriptorFingerprint, PathFingerprintBuffer,
    PublicAbiFingerprint, PublicAbiFingerprintBuffer, SourceFileTable, VerificationMetadata,
    validate_program_resource_limits,
};
pub use instruction::{
    BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, EnumId, FieldRef, FunctionRef,
    HostImportId, JumpTarget, LocalSlot, ModuleSlot, PathId, Register, RuntimeHelper, StructId,
    UnaryOp,
};
pub use lower::{BytecodeLoweringError, lower_program_to_bytecode, lower_to_bytecode};
pub use module::{
    BytecodeDebugMetadata, BytecodeFunction, BytecodeFunctionBuffer, BytecodeInstructionBuffer,
    BytecodeModule, BytecodeModuleSlot, BytecodeModuleSlotBuffer, BytecodeTypeTable,
    CapturedBindingDebugBuffer, CapturedBindingDebugInfo, ConstantPool, ControlFlowTargetBuffer,
    DebugPointId, FrameLayout, FunctionMetadata, FunctionRecord, FunctionTable,
    InstructionSourceSpan, InstructionSourceSpanBuffer, InterfaceMethodSlot, InterfaceTableRecord,
    LineTableBuffer, LineTableEntry, LocalLiveRange, LocalLiveRangeBuffer, PathRecord, PathTable,
    PublicItemRecord, PublicItemTable, SafeDebugPoint, SafeDebugPointBuffer, SafeDebugPointKind,
    TypeLayoutBuffer,
};
pub use program::{BytecodeProgram, ModuleRef, verify_program};
pub use verifier::{BytecodeVerificationError, verify_module};
