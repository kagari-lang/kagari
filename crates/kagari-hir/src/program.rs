//! A checked, snapshot-owned source dependency closure for compilation and linking.

use crate::typeck::TypedFunction;
use std::collections::HashMap;

use kagari_common::{
    diagnostic::{Diagnostic, Severity},
    identity::{
        FileId, ModuleIdentity, Revision, mapping::DefinitionMappingError, table::DefinitionId,
    },
};

use {
    crate::{
        CheckedAnalysis,
        analysis::AnalysisSnapshot,
        imports::{ModuleOrderError, functions::SourceFunctionId},
    },
    kagari_common::cancellation::CancellationToken,
};

#[derive(Debug, Clone)]
pub struct ProgramDiagnostic {
    pub file: FileId,
    pub revision: Revision,
    pub diagnostic: Diagnostic,
}

#[derive(Debug)]
pub enum ProgramCheckError {
    MissingFile(FileId),
    Graph(ModuleOrderError),
    Diagnostics(Vec<ProgramDiagnostic>),
    Cancelled,
    Identity(DefinitionMappingError),
}

/// Members cannot be replaced with facts from another snapshot after checking.
#[derive(Debug, Clone)]
pub struct CheckedProgram {
    root: FileId,
    modules: Vec<CheckedAnalysis>,
    by_file: HashMap<FileId, usize>,
    dependencies: HashMap<ModuleIdentity, Vec<ModuleIdentity>>,
}

impl CheckedProgram {
    pub fn root(&self) -> &CheckedAnalysis {
        &self.modules[self.by_file[&self.root]]
    }

    /// Deterministic order. Diamonds and cycles contain each module once.
    pub fn modules(&self) -> &[CheckedAnalysis] {
        &self.modules
    }

    pub fn dependencies(&self, module: &ModuleIdentity) -> Option<&[ModuleIdentity]> {
        self.dependencies.get(module).map(Vec::as_slice)
    }

    pub fn source_function(
        &self,
        id: SourceFunctionId,
    ) -> Option<(&CheckedAnalysis, &TypedFunction<DefinitionId>)> {
        let module = &self.modules[*self.by_file.get(&id.file)?];
        if module.lowered.source.revision() != id.revision {
            return None;
        }
        let function = module
            .typed
            .functions
            .iter()
            .find(|function| function.id == id.function)?;
        Some((module, function))
    }
}

impl AnalysisSnapshot {
    pub fn check_program(
        &self,
        root: FileId,
        cancel: &CancellationToken,
    ) -> Result<CheckedProgram, ProgramCheckError> {
        cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
        let root_source = self
            .file(root)
            .ok_or(ProgramCheckError::MissingFile(root))?;
        let order = self
            .module_graph()
            .reachable_order(root_source.source().module_identity(), cancel)
            .map_err(|error| match error {
                ModuleOrderError::Cancelled => ProgramCheckError::Cancelled,
                error => ProgramCheckError::Graph(error),
            })?;
        let mut modules = Vec::with_capacity(order.len());
        let mut by_file = HashMap::new();
        let mut dependencies = HashMap::new();
        let mut diagnostics = Vec::new();
        for identity in order {
            cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
            let node = self
                .module_graph()
                .node(&identity)
                .expect("ordered graph node");
            let file = self
                .file(node.file)
                .ok_or(ProgramCheckError::MissingFile(node.file))?;
            for diagnostic in file.result().diagnostics() {
                cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
                if diagnostic.severity == Severity::Error {
                    diagnostics.push(ProgramDiagnostic {
                        file: node.file,
                        revision: file.source().revision(),
                        diagnostic: diagnostic.clone(),
                    });
                }
            }
            by_file.insert(node.file, modules.len());
            dependencies.insert(identity, node.dependencies().to_vec());
            // Source imports are valid here: every reachable module belongs to this
            // immutable snapshot and is checked before the program is returned.
            if !file
                .result()
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error)
            {
                modules.push(
                    CheckedAnalysis::adopt_scoped(
                        file.result().facts(),
                        file.definitions(),
                        cancel,
                    )
                    .map_err(|error| match error {
                        DefinitionMappingError::Cancelled => ProgramCheckError::Cancelled,
                        error => ProgramCheckError::Identity(error),
                    })?,
                );
            }
        }
        cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
        if !diagnostics.is_empty() {
            return Err(ProgramCheckError::Diagnostics(diagnostics));
        }
        Ok(CheckedProgram {
            root,
            modules,
            by_file,
            dependencies,
        })
    }
}
