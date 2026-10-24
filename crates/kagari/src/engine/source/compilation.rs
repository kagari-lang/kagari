//! Same-generation MIR and bytecode ownership for source preparation and export.
use crate::{
    BytecodeArtifact, CompileResult,
    engine::source::{
        ArtifactOptions, CheckedModule, NativeInputExport, inputs::CompilationInputs,
    },
    error::{CompilationPhase, EmbeddingError},
};
use kagari_bytecode::{
    artifact::KbcArtifact, native_input::PortableMir, program::verified::VerifiedBytecodeProgram,
    source_package::VerifiedSourcePackage,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{
        lower::instances::MirLoweringOptions,
        program::{SourceProgramError, lower_program_to_mir, lower_program_to_mir_with_demands},
        source_package::build_source_package,
    },
};
use kagari_contract::{
    types::named::NamedDeclaration,
    updates::{DemandEnvelope, limits::UpdateError},
};
use kagari_mir::{
    codec::{MirCodecError, encode_program},
    program::{ProgramErrorKind, VerifiedMirProgram},
};
use std::sync::Arc;

/// Only lowering can pair these products; callers cannot substitute either half.
pub(crate) struct Compilation {
    bytecode: VerifiedBytecodeProgram,
    mir: VerifiedMirProgram,
    inputs: Arc<CompilationInputs>,
    source: VerifiedSourcePackage,
}

impl Compilation {
    pub(crate) fn lower(
        checked: &CheckedModule,
        options: &MirLoweringOptions,
    ) -> CompileResult<Self> {
        Self::lower_impl(checked, options, None)
    }

    /// Reuses the compiler's retained-demand closure and implementation headers.
    pub(crate) fn lower_with_demands(
        checked: &CheckedModule,
        options: &MirLoweringOptions,
        retained: &DefinitionMetadata<DemandEnvelope>,
        implementation_headers: &[NamedDeclaration],
    ) -> CompileResult<Self> {
        Self::lower_impl(checked, options, Some((retained, implementation_headers)))
    }

    fn lower_impl(
        checked: &CheckedModule,
        options: &MirLoweringOptions,
        retained: Option<(&DefinitionMetadata<DemandEnvelope>, &[NamedDeclaration])>,
    ) -> CompileResult<Self> {
        let inputs = Arc::new(CompilationInputs::capture(
            checked.inputs().clone(),
            options,
        ));
        let options = inputs.lowering(options.cancel.clone());
        let mir = match retained {
            Some((demands, headers)) => {
                lower_program_to_mir_with_demands(checked.program(), &options, demands, headers)
            }
            None => lower_program_to_mir(checked.program(), &options),
        }
        .map_err(|error| match error {
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
        let source = build_source_package(checked.program(), &bytecode, &options.cancel).map_err(
            |error| {
                if options.cancel.check().is_err() {
                    EmbeddingError::Cancelled
                } else {
                    EmbeddingError::Compilation {
                        phase: CompilationPhase::BytecodeLowering,
                        message: format!("exact source package: {error}"),
                    }
                }
            },
        )?;
        Ok(Self {
            source,
            bytecode,
            mir,
            inputs,
        })
    }

    pub(crate) fn inputs(&self) -> &Arc<CompilationInputs> {
        &self.inputs
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        VerifiedBytecodeProgram,
        VerifiedMirProgram,
        VerifiedSourcePackage,
    ) {
        (self.bytecode, self.mir, self.source)
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
        let artifact = KbcArtifact::from_verified(self.bytecode, build, &options.lowering.cancel)
            .and_then(|artifact| {
                if options.retain_source {
                    KbcArtifact::with_source_package(
                        artifact,
                        &self.source,
                        &options.lowering.cancel,
                    )
                } else {
                    Ok(artifact)
                }
            });
        artifact.map_err(|error| {
            if options.lowering.cancel.check().is_err() {
                EmbeddingError::Cancelled
            } else {
                EmbeddingError::artifact_validation(error)
            }
        })
    }
}
