//! One retained analysis handoff shared by checking, linting and compilation.

use crate::{
    CompileResult,
    engine::{packages::PackageBuildInput, source::CheckedModule},
    error::EmbeddingError,
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::{
    analysis::AnalysisSnapshot, imports::ModuleOrderError, program::ProgramCheckError,
};
use kagari_source::{identity::FileId, presentation::DiagnosticRecord};

/// A recoverable root analysis under the engine's sealed native declarations.
/// Retains the same facts for optional tools and strict checking; changing tool
/// severity does not parse or type-check source again.
#[derive(Debug)]
pub struct SourceAnalysis {
    pub(super) snapshot: AnalysisSnapshot,
    pub(super) root: FileId,
    pub(super) package_inputs: Option<PackageBuildInput>,
}

impl SourceAnalysis {
    pub fn snapshot(&self) -> &AnalysisSnapshot {
        &self.snapshot
    }

    pub fn root(&self) -> FileId {
        self.root
    }

    /// Requires an error-free closure using the existing HIR checked-program gate.
    /// No parsing, type checking, lowering or executable emission is repeated.
    pub fn check(&self, cancel: &CancellationToken) -> CompileResult<CheckedModule> {
        let analysis = self
            .snapshot
            .file(self.root)
            .ok_or_else(|| EmbeddingError::Source {
                message: "file is absent from this source snapshot".into(),
            })?;
        let source = analysis.source();

        let snapshot = &self.snapshot;
        let program = snapshot
            .check_program(self.root, cancel)
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
                            DiagnosticRecord::from_diagnostic(
                                record.diagnostic,
                                snapshot.file(record.file).expect("checked source").source(),
                            )
                        })
                        .collect(),
                },
                ProgramCheckError::Graph(error) => {
                    let failed = match error {
                        ModuleOrderError::Cancelled => return EmbeddingError::Cancelled,
                        ModuleOrderError::Identity(error) => {
                            return EmbeddingError::Source {
                                message: format!("invalid module identity: {error}"),
                            };
                        }
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
                            .node(
                                &snapshot
                                    .module_graph()
                                    .catalog
                                    .definitions
                                    .lookup_module(&module)
                                    .expect("failed module identity"),
                            )
                            .expect("failed graph node");
                        let file = snapshot.file(node.file).expect("graph source");
                        diagnostics.extend(node.imports.diagnostics.iter().cloned().map(
                            |diagnostic| {
                                DiagnosticRecord::from_diagnostic(diagnostic, file.source())
                            },
                        ));
                    }
                    EmbeddingError::Diagnostics { diagnostics }
                }
            })?;
        Ok(CheckedModule {
            source_name: source.name().to_owned(),
            program,
            package_inputs: self.package_inputs.clone(),
        })
    }
}
