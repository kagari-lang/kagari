//! Same-generation MIR and bytecode ownership for source preparation and export.
use crate::{
    BytecodeArtifact, CompileResult,
    engine::source::{ArtifactOptions, CheckedModule, NativeInputExport},
    error::{CompilationPhase, EmbeddingError},
};
use kagari_bytecode::{
    artifact::KbcArtifact, native_input::PortableMir, program::verified::VerifiedBytecodeProgram,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{
        lower::instances::MirLoweringOptions,
        program::{SourceProgramError, lower_program_to_mir},
    },
};
use kagari_contract::updates::limits::UpdateError;
use kagari_mir::{
    codec::{MirCodecError, encode_program},
    program::{ProgramErrorKind, VerifiedMirProgram},
};

/// Only lowering can pair these products; callers cannot substitute either half.
pub(crate) struct Compilation {
    bytecode: VerifiedBytecodeProgram,
    mir: VerifiedMirProgram,
}

impl Compilation {
    pub(crate) fn lower(
        checked: &CheckedModule,
        options: &MirLoweringOptions,
    ) -> CompileResult<Self> {
        let mir =
            lower_program_to_mir(checked.program(), options).map_err(|error| match error {
                SourceProgramError::Lowering { module, error } => {
                    let source = &checked
                        .program()
                        .modules()
                        .iter()
                        .find(|item| item.lowered.source.module_identity() == module.as_ref())
                        .expect("lowered program source")
                        .lowered
                        .source;
                    EmbeddingError::ir_lowering(error, source)
                }
                SourceProgramError::Update { stage, error } => match error {
                    UpdateError::Cancelled => EmbeddingError::Cancelled,
                    error => EmbeddingError::Compilation {
                        phase: CompilationPhase::MirLowering,
                        message: format!("{stage}: {error}"),
                    },
                },
                SourceProgramError::Verification(error) => match error.kind {
                    ProgramErrorKind::Cancelled => EmbeddingError::Cancelled,
                    kind => EmbeddingError::Compilation {
                        phase: CompilationPhase::MirLowering,
                        message: format!("{}: {kind:?}", error.module),
                    },
                },
            })?;
        let bytecode = lower_program_to_bytecode(&mir, &options.cancel).map_err(|error| {
            if options.cancel.check().is_err() {
                EmbeddingError::Cancelled
            } else {
                EmbeddingError::bytecode_lowering(error)
            }
        })?;
        Ok(Self { bytecode, mir })
    }

    pub(crate) fn into_parts(self) -> (VerifiedBytecodeProgram, VerifiedMirProgram) {
        (self.bytecode, self.mir)
    }

    pub(crate) fn export(
        self,
        options: ArtifactOptions,
    ) -> CompileResult<DefinitionMetadata<BytecodeArtifact>> {
        let mut build = options.build;
        build.portable_mir = match options.native_input {
            NativeInputExport::PortableMir => Some(PortableMir {
                bytes: encode_program(&self.mir, &options.lowering.cancel).map_err(|error| {
                    match error {
                        MirCodecError::Cancelled => EmbeddingError::Cancelled,
                        error => EmbeddingError::Compilation {
                            phase: CompilationPhase::ArtifactEncoding,
                            message: error.to_string(),
                        },
                    }
                })?,
            }),
            NativeInputExport::BytecodeOnly => None,
        };
        KbcArtifact::from_verified(self.bytecode, build, &options.lowering.cancel).map_err(
            |error| {
                if options.lowering.cancel.check().is_err() {
                    EmbeddingError::Cancelled
                } else {
                    EmbeddingError::artifact_validation(error)
                }
            },
        )
    }
}
