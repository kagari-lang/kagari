//! Source analysis and artifact emission are optional SDK capabilities.
use crate::{
    BytecodeArtifact, CompileResult,
    engine::{KagariEngine, packages::PackageBuildInput},
    error::{CompilationPhase, EmbeddingDiagnostic, EmbeddingError},
};

use blake3::Hasher;
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KAGARI_COMPILER_FINGERPRINT, KbcArtifact},
    native_input::PortableMir,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata, table::DefinitionId,
    },
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
    passes::PassOptions,
    program::ProgramErrorKind,
};
use kagari_package::{error::PackageError, snapshot::ProgramInputs};
use kagari_source::{
    identity::FileId,
    source::SourceFile,
    source_database::{SourceLayer, SourceSnapshot},
};
use kagari_syntax::parser::ParseLimits;
use kagari_types::host_interface::HostInterfaceError;
use std::sync::Arc;

fn analysis_error(error: AnalysisError) -> EmbeddingError {
    match error {
        AnalysisError::Cancelled => EmbeddingError::Cancelled,
        AnalysisError::Identity(error) => EmbeddingError::Source {
            message: format!("invalid analysis definition metadata: {error}"),
        },
        AnalysisError::Imports(error) => EmbeddingError::Source {
            message: error.to_string(),
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
    package_inputs: Option<PackageBuildInput>,
}

impl CheckedModule {
    pub fn module_identity(&self) -> &ModuleIdentity {
        self.program.root().lowered.source.module_identity()
    }

    pub fn program(&self) -> &CheckedProgram {
        &self.program
    }

    /// Exact captured source closure when compiled through `compile_package`.
    ///
    /// Retains source bytes, dependency aliases and selections independently of
    /// later source edits or engine queries. Manually supplied source snapshots
    /// have no acquisition provenance and return `None`.
    pub fn package_inputs(&self) -> Option<&ProgramInputs> {
        self.package_inputs.as_ref().map(PackageBuildInput::sources)
    }

    /// The checked package's source, provider and query-setting input identity.
    /// Lowering options and emitted bytes are separate artifact-storage inputs.
    pub fn package_fingerprint(&self) -> Option<&str> {
        self.package_inputs
            .as_ref()
            .map(PackageBuildInput::fingerprint)
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactOptions {
    pub build: ArtifactBuildOptions,
    pub lowering: MirLoweringOptions,
    /// Controls native input generated from the same verified MIR as bytecode.
    /// Supersedes any opaque payload in `build.portable_mir`.
    pub native_input: NativeInputExport,
}

impl Default for ArtifactOptions {
    fn default() -> Self {
        Self {
            build: ArtifactBuildOptions::default(),
            lowering: MirLoweringOptions {
                optimization: Some(PassOptions::default()),
                ..MirLoweringOptions::default()
            },
            native_input: NativeInputExport::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NativeInputExport {
    #[default]
    PortableMir,
    /// This artifact cannot be compiled natively without fresh compiler input.
    BytecodeOnly,
}

impl KagariEngine {
    /// Identifies captured package inputs in this engine's compilation environment.
    ///
    /// Includes exact source selections/bytes/aliases, complete installed native and
    /// offline Host declarations, parser/constant/diagnostic limits and the compiler
    /// contract identifier. It excludes runtime state/callback captures and physical
    /// tooling origins. This does not check source or validate an executable product.
    /// Lowering options and emitted artifact bytes must also participate in product
    /// storage; the identifier alone never authorizes skipping artifact validation.
    pub fn package_fingerprint(
        &self,
        inputs: &ProgramInputs,
        cancel: &CancellationToken,
    ) -> CompileResult<String> {
        let environment = self
            .analysis
            .borrow()
            .environment_fingerprint(cancel)
            .map_err(analysis_error)?;
        let mut hash = Hasher::new_derive_key("kagari package compiler input v1");
        for field in [
            inputs.fingerprint(),
            environment.as_str(),
            KAGARI_COMPILER_FINGERPRINT,
        ] {
            hash.update(&(field.len() as u64).to_le_bytes());
            hash.update(field.as_bytes());
        }
        Ok(hash.finalize().to_hex().to_string())
    }

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

    /// Install a scoped offline contract, optionally with declaration/Rust origins
    /// for tooling. Interface metadata alone carries no source locations.
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

    /// Checks one captured package and its complete declared dependency closure.
    ///
    /// Local and host virtual packages share this entrypoint. Acquisition and lock
    /// publication belong to `kagari-package`; this method reads no filesystem and
    /// performs no execution. It replaces this session's sources atomically, reusing
    /// unchanged file identities. Earlier checked results and source snapshots keep
    /// their original inputs. Installed native and offline host declarations remain
    /// inputs of this engine. Use separate sessions for independently edited roots.
    ///
    /// Returns cancellation, invalid captured-source input or ordinary source
    /// diagnostics. Captured-source validation or cancellation publishes no partial
    /// batch; semantic diagnostics leave the captured sources available for tooling.
    pub fn compile_package(
        &self,
        inputs: &ProgramInputs,
        cancel: &CancellationToken,
    ) -> CompileResult<CheckedModule> {
        let source = inputs
            .source_snapshot(&mut self.sources.borrow_mut(), cancel)
            .map_err(|error| match error {
                PackageError::Cancelled => EmbeddingError::Cancelled,
                error => EmbeddingError::Source {
                    message: error.to_string(),
                },
            })?;
        let root = ModuleIdentity {
            package: inputs.root().family.package_id(),
            path: Vec::new(),
        };
        let file = source
            .module(&root)
            .ok_or_else(|| EmbeddingError::Source {
                message: format!("captured package root is absent: {root}"),
            })?
            .id();
        let mut checked = self.compile_snapshot(source, file, cancel)?;
        checked.package_inputs = Some(PackageBuildInput::capture(self, inputs, cancel)?);
        Ok(checked)
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
    pub fn body(
        &self,
        source: SourceSnapshot,
        function: &DefinitionId,
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
                ProgramCheckError::Exports(message) => EmbeddingError::Source { message },
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
            package_inputs: None,
        })
    }

    pub fn emit_bytecode(
        &self,
        checked: &CheckedModule,
        options: ArtifactOptions,
    ) -> CompileResult<DefinitionMetadata<BytecodeArtifact>> {
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
        let context =
            DefinitionContext::new().map_err(|error| artifact_identity_error(error.into()))?;
        let program = program
            .import_into(&context, &options.lowering.cancel)
            .map_err(artifact_identity_error)?;
        let artifact = KbcArtifact::from_program(program.into_records(), build, &context)
            .map_err(EmbeddingError::artifact_validation)?;
        DefinitionMetadata::checked(context.snapshot(), artifact, &options.lowering.cancel)
            .map_err(artifact_identity_error)
    }

    pub fn compile_to_artifact(
        &self,
        source: SourceFile,
        artifact_options: ArtifactOptions,
    ) -> CompileResult<DefinitionMetadata<BytecodeArtifact>> {
        let checked = self.compile_source(source)?;
        self.emit_bytecode(&checked, artifact_options)
    }
}

fn artifact_identity_error(error: DefinitionMappingError) -> EmbeddingError {
    match error {
        DefinitionMappingError::Cancelled => EmbeddingError::Cancelled,
        error => EmbeddingError::artifact_validation(error.into()),
    }
}
