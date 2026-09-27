mod access;

mod artifact;
mod instruction;
mod module;
mod program;
mod trait_bounds;
pub use trait_bounds::interface_ancestors;
mod verifier;

pub use artifact::{
    ArtifactBuildOptions, ArtifactCodecError, ArtifactCompatibility, ArtifactEncoding,
    ArtifactFingerprint, ArtifactHeader, ArtifactSection, ArtifactSectionBuffer, ArtifactSectionId,
    ArtifactSignature, ArtifactSignatures, ArtifactTables, ArtifactValidationError,
    ControlFlowTargetMetadata, ControlFlowTargetMetadataBuffer, DebugMetadata, DebugNameTable,
    DependencyFingerprint, DependencyFingerprintBuffer, FunctionEffectBuffer,
    FunctionEffectMetadata, FunctionLayoutBuffer, FunctionLayoutMetadata,
    KAGARI_COMPILER_FINGERPRINT, KAGARI_LANGUAGE_VERSION, KBC_ARTIFACT_FORMAT_VERSION, KBC_MAGIC,
    KbcArtifact, LoaderValidationMetadata, ModuleEpoch, PathDescriptorFingerprint,
    PathFingerprintBuffer, PublicAbiFingerprint, PublicAbiFingerprintBuffer, SourceFileTable,
    VerificationMetadata, validate_program_resource_limits,
};
pub use instruction::BinaryOp;
pub use instruction::BytecodeInstruction;
pub use instruction::CallTarget;
pub use instruction::ConstantOperand;
pub use instruction::EnumId;
pub use instruction::FieldRef;
pub use instruction::HostImportId;
pub use instruction::InterfaceTableRef;
pub use instruction::JumpTarget;
pub use instruction::LocalSlot;
pub use instruction::ModuleSlot;
pub use instruction::PathId;
pub use instruction::Register;
pub use instruction::RuntimeHelper;
pub use instruction::StructId;
pub use instruction::UnaryOp;
pub use module::BytecodeDebugMetadata;
pub use module::BytecodeFunction;
pub use module::BytecodeFunctionBuffer;
pub use module::BytecodeInstructionBuffer;
pub use module::BytecodeModule;
pub use module::BytecodeModuleSlot;
pub use module::BytecodeModuleSlotBuffer;
pub use module::BytecodeTypeTable;
pub use module::CapturedBindingDebugBuffer;
pub use module::CapturedBindingDebugInfo;
pub use module::ConstantPool;
pub use module::ControlFlowTargetBuffer;
pub use module::FrameLayout;
pub use module::FunctionMetadata;
pub use module::FunctionRecord;
pub use module::FunctionTable;
pub use module::InstructionSourceSpan;
pub use module::InstructionSourceSpanBuffer;
pub use module::InterfaceMethodSlot;
pub use module::InterfaceTableRecord;
pub use module::LineTableBuffer;
pub use module::LineTableEntry;
pub use module::LocalLiveRange;
pub use module::LocalLiveRangeBuffer;
pub use module::PathRecord;
pub use module::PathTable;
pub use module::PublicItemRecord;
pub use module::PublicItemTable;
pub use module::RootSlotLayout;
pub use module::SafeDebugPoint;
pub use module::SafeDebugPointBuffer;
pub use module::SafeDebugPointKind;
pub use module::TypeLayoutBuffer;
pub use program::{BytecodeProgram, ModuleRef, verify_program};
pub use verifier::{BytecodeVerificationError, verify_module};
