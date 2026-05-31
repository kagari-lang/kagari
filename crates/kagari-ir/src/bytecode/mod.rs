pub use kagari_hir::builtin::BuiltinMethod;

mod artifact;
mod instruction;
mod lower;
mod module;
mod verifier;

pub use artifact::{
    ArtifactBuildOptions, ArtifactCompatibility, ArtifactEncoding, ArtifactFingerprint,
    ArtifactHeader, ArtifactModuleIdentity, ArtifactSection, ArtifactSectionBuffer,
    ArtifactSectionId, ArtifactSignature, ArtifactSignatures, ArtifactTables,
    ArtifactValidationError, ControlFlowTargetMetadata, ControlFlowTargetMetadataBuffer,
    DebugMetadata, DebugNameTable, DependencyFingerprint, DependencyFingerprintBuffer,
    FunctionEffectBuffer, FunctionEffectMetadata, FunctionLayoutBuffer, FunctionLayoutMetadata,
    HostDependencyTable, KAGARI_COMPILER_FINGERPRINT, KAGARI_LANGUAGE_VERSION,
    KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION, KBC_ARTIFACT_FORMAT_VERSION,
    KBC_MAGIC, KbcArtifact, LoaderValidationMetadata, ModuleEpoch, PathDescriptorFingerprint,
    PathFingerprintBuffer, PublicAbiFingerprint, PublicAbiFingerprintBuffer, SourceFileTable,
    VerificationMetadata,
};
pub use instruction::{
    BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, FieldId, FunctionRef, JumpTarget,
    LocalSlot, ModuleSlot, PathId, Register, RuntimeHelper, StructFieldInit, UnaryOp,
};
pub use lower::{BytecodeLoweringError, lower_to_bytecode};
pub use module::{
    BytecodeFunction, BytecodeFunctionBuffer, BytecodeInstructionBuffer, BytecodeModule,
    BytecodeModuleSlot, BytecodeModuleSlotBuffer, BytecodeTypeTable, ConstantPool,
    ControlFlowTargetBuffer, FieldRecord, FieldTable, FunctionMetadata, FunctionRecord,
    FunctionTable, PathRecord, PathTable, PublicItemRecord, PublicItemTable, TypeLayoutBuffer,
};
pub use verifier::{BytecodeVerificationError, verify_module};
