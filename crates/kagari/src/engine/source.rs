//! Optional source analysis, direct executable preparation and artifact export.

pub mod analysis;
pub(crate) mod compilation;

use crate::{
    BytecodeArtifact, CompileResult,
    engine::{
        KagariEngine,
        packages::PackageBuildInput,
        source::{analysis::SourceAnalysis, compilation::Compilation},
    },
    error::EmbeddingError,
    program::PreparedProgram,
};

use blake3::Hasher;
use kagari_bytecode::artifact::{ArtifactBuildOptions, KAGARI_COMPILER_FINGERPRINT};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, metadata::DefinitionMetadata, table::DefinitionId},
};
use kagari_compiler::source::lower::instances::MirLoweringOptions;
use kagari_hir::{
    analysis::{
        AnalysisSnapshot, body_queries::FunctionAnalysis, declaration_queries::DeclarationSnapshot,
        error::AnalysisError, signature_queries::SignatureSnapshot,
    },
    program::CheckedProgram,
    typeck::const_budget::ConstLimits,
};
use kagari_mir::passes::PassOptions;
use kagari_package::{error::PackageError, snapshot::ProgramInputs};
use kagari_source::{
    identity::FileId,
    source::SourceFile,
    source_database::{SourceLayer, SourceSnapshot},
};
use kagari_syntax::parser::ParseLimits;
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

/// Controls one source preparation request independently of Cargo features.
///
/// Bounded MIR simplification is enabled by default. Native support being built
/// into the SDK does not retain compiler input unless this request asks for it.
#[derive(Debug, Clone)]
pub struct PreparationOptions {
    pub lowering: MirLoweringOptions,
    #[cfg(feature = "native")]
    pub retain_native_input: bool,
}

impl Default for PreparationOptions {
    fn default() -> Self {
        Self {
            lowering: MirLoweringOptions {
                optimization: Some(PassOptions::default()),
                ..MirLoweringOptions::default()
            },
            #[cfg(feature = "native")]
            retain_native_input: false,
        }
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

    pub fn compile_source(&self, source: SourceFile) -> CompileResult<CheckedModule> {
        self.analyze_source(source, &CancellationToken::default())?
            .check(&CancellationToken::default())
    }

    /// Retains source facts and diagnostics for checking and optional source tools.
    pub fn analyze_source(
        &self,
        source: SourceFile,
        cancel: &CancellationToken,
    ) -> CompileResult<SourceAnalysis> {
        let root = self.set_source(source.name(), source.text().to_owned(), SourceLayer::Base)?;
        Ok(SourceAnalysis {
            snapshot: self.analyze(self.source_snapshot(), cancel)?,
            root,
            package_inputs: None,
        })
    }

    /// Checks one captured package and its complete declared dependency closure.
    ///
    /// Local and host virtual packages share this entrypoint. Acquisition and lock
    /// publication belong to `kagari-package`; this method reads no filesystem and
    /// performs no execution. It replaces this session's sources atomically, reusing
    /// unchanged file identities. Earlier checked results and source snapshots keep
    /// their original inputs. The engine's sealed native declarations remain
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
        self.analyze_package(inputs, cancel)?.check(cancel)
    }

    /// Captures a complete package closure and retains recoverable semantic facts.
    /// Source completeness remains a package-owned prerequisite; errors in bodies
    /// remain diagnostics in the returned analysis rather than executable evidence.
    pub fn analyze_package(
        &self,
        inputs: &ProgramInputs,
        cancel: &CancellationToken,
    ) -> CompileResult<SourceAnalysis> {
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
        Ok(SourceAnalysis {
            snapshot: self.analyze(source, cancel)?,
            root: file,
            package_inputs: Some(PackageBuildInput::capture(self, inputs, cancel)?),
        })
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
        SourceAnalysis {
            snapshot: self.analyze(source, cancel)?,
            root: file,
            package_inputs: None,
        }
        .check(cancel)
    }

    /// Lower captured checked source directly into reusable executable input.
    ///
    /// The result owns its identities and debug facts independently of this engine.
    /// Runtime installation still checks host bindings and reload compatibility.
    pub fn prepare(
        &self,
        checked: &CheckedModule,
        options: PreparationOptions,
    ) -> CompileResult<PreparedProgram> {
        let compilation = Compilation::lower(checked, &options.lowering)?;
        let prepared = PreparedProgram::from_compilation(compilation, &options);
        options
            .lowering
            .cancel
            .check()
            .map_err(|_| EmbeddingError::Cancelled)?;
        Ok(prepared)
    }

    /// Check and prepare source for execution without constructing an artifact.
    pub fn compile_to_program(
        &self,
        source: SourceFile,
        options: PreparationOptions,
    ) -> CompileResult<PreparedProgram> {
        let checked = self
            .analyze_source(source, &options.lowering.cancel)?
            .check(&options.lowering.cancel)?;
        self.prepare(&checked, options)
    }

    /// Explicitly export checked source as an editable portable artifact.
    pub fn emit_artifact(
        &self,
        checked: &CheckedModule,
        options: ArtifactOptions,
    ) -> CompileResult<DefinitionMetadata<BytecodeArtifact>> {
        Compilation::lower(checked, &options.lowering)?.export(options)
    }

    pub fn compile_to_artifact(
        &self,
        source: SourceFile,
        artifact_options: ArtifactOptions,
    ) -> CompileResult<DefinitionMetadata<BytecodeArtifact>> {
        let checked = self
            .analyze_source(source, &artifact_options.lowering.cancel)?
            .check(&artifact_options.lowering.cancel)?;
        self.emit_artifact(&checked, artifact_options)
    }
}
