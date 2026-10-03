//! Source analysis and artifact emission are optional SDK capabilities.
use crate::{
    BytecodeArtifact, CompileResult,
    engine::KagariEngine,
    error::{CompilationPhase, EmbeddingDiagnostic, EmbeddingError},
};

use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    native_input::PortableMir,
};
use kagari_common::{
    cancellation::CancellationToken,
    host_interface::HostInterfaceError,
    identity::{FileId, ModuleIdentity, reference::DefinitionReference},
    source::SourceFile,
    source_database::{SourceLayer, SourceSnapshot},
};
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{
        lower::instances::MirLoweringOptions,
        program::{SourceProgramError, lower_program_to_mir},
    },
};
use kagari_hir::{
    analysis::{
        AnalysisSnapshot, body_queries::FunctionAnalysis, declaration_queries::DeclarationSnapshot,
        error::AnalysisError, signature_queries::SignatureSnapshot,
    },
    host::{HostDeclarations, origin::HostInput},
    imports::ModuleOrderError,
    program::{CheckedProgram, ProgramCheckError},
    typeck::const_budget::ConstLimits,
};
use kagari_mir::{
    codec::{MirCodecError, encode_program},
    program::ProgramErrorKind,
};

use kagari_syntax::parser::ParseLimits;
use std::sync::Arc;

fn analysis_error(error: AnalysisError) -> EmbeddingError {
    match error {
        AnalysisError::Cancelled => EmbeddingError::Cancelled,
        AnalysisError::Identity(error) => EmbeddingError::Source {
            message: format!("invalid analysis definition metadata: {error}"),
        },
        AnalysisError::NativeApi(error) => EmbeddingError::Source {
            message: error.to_string(),
        },
    }
}

#[derive(Debug)]
pub struct CheckedModule {
    pub source_name: String,
    program: CheckedProgram,
}

impl CheckedModule {
    pub fn module_identity(&self) -> &ModuleIdentity {
        self.program.root().lowered.source.module_identity()
    }

    pub fn program(&self) -> &CheckedProgram {
        &self.program
    }
}

#[derive(Debug, Clone, Default)]
pub struct ArtifactOptions {
    pub build: ArtifactBuildOptions,
    pub lowering: MirLoweringOptions,
    /// Controls native input generated from the same verified MIR as bytecode.
    /// Supersedes any opaque payload in `build.portable_mir`.
    pub native_input: NativeInputExport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NativeInputExport {
    #[default]
    PortableMir,
    /// This artifact cannot be compiled natively without fresh compiler input.
    BytecodeOnly,
}

impl KagariEngine {
    pub fn set_const_limits(&self, limits: ConstLimits) {
        self.analysis.borrow_mut().set_const_limits(limits);
    }

    pub fn set_parse_limits(&self, limits: ParseLimits) {
        self.analysis.borrow_mut().set_parse_limits(limits);
    }

    pub fn set_max_semantic_diagnostics(&self, limit: usize) {
        self.analysis
            .borrow_mut()
            .set_max_semantic_diagnostics(limit);
    }

    /// Install an offline contract, optionally with declaration/Rust origins for
    /// tooling. A plain HostInterface carries no source locations.
    pub fn set_host_interface(
        &self,
        input: impl Into<HostInput>,
    ) -> Result<(), HostInterfaceError> {
        let declarations = HostDeclarations::new(input)?;
        self.analysis
            .borrow_mut()
            .set_host_declarations(declarations);
        Ok(())
    }

    pub fn compile_source(&self, source: SourceFile) -> CompileResult<CheckedModule> {
        let id = self.set_source(source.name(), source.text().to_owned(), SourceLayer::Base)?;
        self.compile_snapshot(self.source_snapshot(), id, &CancellationToken::default())
    }

    pub fn set_source(
        &self,
        name: &str,
        text: String,
        layer: SourceLayer,
    ) -> CompileResult<FileId> {
        self.sources
            .borrow_mut()
            .set(name, text, layer)
            .map_err(|message| EmbeddingError::Source { message })
    }

    pub fn load_source(&self, path: &str) -> CompileResult<FileId> {
        self.sources
            .borrow_mut()
            .load_file(path)
            .map_err(|message| EmbeddingError::Source { message })
    }

    pub fn bind_module(&self, name: &str, module: ModuleIdentity) -> CompileResult<FileId> {
        self.sources
            .borrow_mut()
            .bind_module(name, module)
            .map_err(|message| EmbeddingError::Source { message })
    }

    pub fn close_overlay(&self, name: &str) -> CompileResult<()> {
        self.sources
            .borrow_mut()
            .close_overlay(name)
            .map_err(|message| EmbeddingError::Source { message })
    }

    pub fn source_snapshot(&self) -> SourceSnapshot {
        self.sources.borrow().snapshot()
    }

    pub fn analyze(
        &self,
        source: SourceSnapshot,

        cancel: &CancellationToken,
    ) -> CompileResult<AnalysisSnapshot> {
        self.analysis
            .borrow_mut()
            .snapshot(source, cancel)
            .map_err(analysis_error)
    }

    /// Parse and collect module declarations without resolving or checking bodies.
    pub fn declarations(
        &self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> CompileResult<DeclarationSnapshot> {
        self.analysis
            .borrow_mut()
            .declarations(source, cancel)
            .map_err(analysis_error)
    }

    /// Check declaration signatures without resolving or checking function bodies.
    pub fn signatures(
        &self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> CompileResult<SignatureSnapshot> {
        self.analysis
            .borrow_mut()
            .signatures(source, cancel)
            .map_err(analysis_error)
    }

    /// Query one function body and module-constant prerequisites by declaration identity.
    pub fn body<I: DefinitionReference>(
        &self,
        source: SourceSnapshot,
        function: &I,
        cancel: &CancellationToken,
    ) -> CompileResult<Option<Arc<FunctionAnalysis>>> {
        self.analysis
            .borrow_mut()
            .body(source, function, cancel)
            .map_err(analysis_error)
    }

    pub fn compile_snapshot(
        &self,
        source: SourceSnapshot,
        file: FileId,

        cancel: &CancellationToken,
    ) -> CompileResult<CheckedModule> {
        let snapshot = self.analyze(source, cancel)?;
        let analysis = snapshot.file(file).ok_or_else(|| EmbeddingError::Source {
            message: "file is absent from this source snapshot".into(),
        })?;
        let source = analysis.source();

        let program = snapshot
            .check_program(file, cancel)
            .map_err(|error| match error {
                ProgramCheckError::Cancelled => EmbeddingError::Cancelled,
                ProgramCheckError::Identity(error) => EmbeddingError::Source {
                    message: format!("invalid analysis definition metadata: {error}"),
                },
                ProgramCheckError::MissingFile(file) => EmbeddingError::Source {
                    message: format!("missing source file {file:?}"),
                },
                ProgramCheckError::Diagnostics(records) => EmbeddingError::Diagnostics {
                    diagnostics: records
                        .into_iter()
                        .map(|record| {
                            EmbeddingDiagnostic::from_diagnostic(
                                record.diagnostic,
                                snapshot.file(record.file).expect("checked source").source(),
                            )
                        })
                        .collect(),
                },
                ProgramCheckError::Graph(error) => {
                    let failed = match error {
                        ModuleOrderError::Cancelled => return EmbeddingError::Cancelled,
                        ModuleOrderError::InvalidImports(module) => vec![module],
                        ModuleOrderError::Missing(module) => {
                            return EmbeddingError::Source {
                                message: format!("missing source module {module}"),
                            };
                        }
                    };
                    let mut diagnostics = Vec::new();
                    for module in failed {
                        let node = snapshot
                            .module_graph()
                            .node(&module)
                            .expect("failed graph node");
                        let file = snapshot.file(node.file).expect("graph source");
                        diagnostics.extend(node.imports.diagnostics.iter().cloned().map(
                            |diagnostic| {
                                EmbeddingDiagnostic::from_diagnostic(diagnostic, file.source())
                            },
                        ));
                    }
                    EmbeddingError::Diagnostics { diagnostics }
                }
            })?;
        Ok(CheckedModule {
            source_name: source.name().to_owned(),
            program,
        })
    }

    pub fn emit_bytecode(
        &self,
        checked: &CheckedModule,
        options: ArtifactOptions,
    ) -> CompileResult<BytecodeArtifact> {
        let ir =
            lower_program_to_mir(&checked.program, &options.lowering).map_err(
                |error| match error {
                    SourceProgramError::Lowering { module, error } => {
                        let source = &checked
                            .program
                            .modules()
                            .iter()
                            .find(|item| item.lowered.source.module_identity() == module.as_ref())
                            .expect("lowered program source")
                            .lowered
                            .source;
                        EmbeddingError::ir_lowering(error, source)
                    }
                    SourceProgramError::Verification(error) => match error.kind {
                        ProgramErrorKind::Cancelled => EmbeddingError::Cancelled,
                        kind => EmbeddingError::Compilation {
                            phase: CompilationPhase::MirLowering,
                            message: format!("{}: {kind:?}", error.module),
                        },
                    },
                },
            )?;
        let program = lower_program_to_bytecode(&ir).map_err(EmbeddingError::bytecode_lowering)?;
        let mut build = options.build;
        build.portable_mir = match options.native_input {
            NativeInputExport::PortableMir => Some(PortableMir {
                bytes: encode_program(&ir, &options.lowering.cancel).map_err(
                    |error| match error {
                        MirCodecError::Cancelled => EmbeddingError::Cancelled,
                        error => EmbeddingError::Compilation {
                            phase: CompilationPhase::ArtifactEncoding,
                            message: error.to_string(),
                        },
                    },
                )?,
            }),
            NativeInputExport::BytecodeOnly => None,
        };
        KbcArtifact::from_program(program, build).map_err(EmbeddingError::artifact_validation)
    }

    pub fn compile_to_artifact(
        &self,
        source: SourceFile,

        artifact_options: ArtifactOptions,
    ) -> CompileResult<BytecodeArtifact> {
        let checked = self.compile_source(source)?;
        self.emit_bytecode(&checked, artifact_options)
    }
}
