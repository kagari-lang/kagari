//! Signature queries stop before body analysis; body environments additionally
//! prepare checked constant prerequisites before projecting imported values.

use crate::{
    AnalysisResult, DiagnosticBuffer, PreparedSignatures,
    aggregates::AggregateCatalog,
    analysis::{
        AnalysisDatabase,
        declarations::{DeclarationSnapshot, FileDeclarations},
        error::AnalysisError,
        ownership,
    },
    collections::HashMap,
    declarations::Declarations,
    hir::ids::BodySelection,
    imports::{
        ModuleGraph,
        constants::ImportedConstants,
        functions::{FunctionCatalog, ImportedFunctions},
        statics::ImportedStatics,
        types::TypeCatalog,
    },
    lower::LoweredModule,
    resolver::collect::resolve_bodies,
    semantic::signatures::BodySignatures,
    typeck::{
        BodyInputs, ModuleSignatures,
        check::{check_bodies_controlled, prepare_body_signatures},
        const_budget::ConstLimits,
        constant_facts::CheckedConstants,
    },
};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{metadata::DefinitionMetadata, table::DefinitionTable},
};
use kagari_source::{
    diagnostic::Diagnostic,
    identity::{FileId, Revision},
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_syntax::ast::module::ModuleSyntax;
use std::{collections::BTreeMap, sync::Arc};

#[cfg(test)]
mod tests;

/// One file's declaration snapshot plus checked signature facts and accumulated diagnostics.
#[derive(Debug, Clone)]
pub struct FileSignatures {
    pub(super) declaration: Arc<FileDeclarations>,
    pub(super) prepared: PreparedSignatures,
    /// Returns accumulated diagnostics through the signature stage, excluding function bodies.
    diagnostics: Arc<DiagnosticBuffer>,
}

impl FileSignatures {
    /// Borrows the exact lowering owning all signature HIR IDs and origins.
    pub fn lowered(&self) -> &LoweredModule {
        &self.prepared.lowered
    }

    /// Returns matching source syntax without preparing bodies; structured inputs have none.
    pub fn syntax(&self) -> Option<ModuleSyntax> {
        self.declaration.syntax()
    }

    /// Borrows the source retained by the declaration stage.
    pub fn source(&self) -> &SourceFile {
        self.declaration.source()
    }

    /// Borrows declaration identities and imported type bindings used by signature checking.
    pub fn declarations(&self) -> &Declarations {
        &self.prepared.record.declarations
    }

    /// Borrows checked signatures and their recoverable signature diagnostics.
    pub fn signatures(&self) -> &Arc<AnalysisResult<ModuleSignatures>> {
        &self.prepared.record.signatures
    }

    /// Returns accumulated diagnostics through the signature stage, excluding function bodies.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// Checked signatures for all declaration files, before function-body checking.
///
/// Retains a [`DeclarationSnapshot`], a `FileId -> Arc<FileSignatures>` map and a
/// shared aggregate catalog. Body queries derive visible function/aggregate inputs
/// from this snapshot. Completing aggregate contracts can add signature diagnostics.
#[derive(Debug, Clone)]
pub struct SignatureSnapshot {
    pub(super) declarations: DeclarationSnapshot,
    pub(super) files: Arc<BTreeMap<FileId, Arc<FileSignatures>>>,
    aggregates: Arc<AggregateCatalog>,
    definitions: DefinitionTable,
}

impl SignatureSnapshot {
    /// Shared parser/name/signature validity, before body diagnostic presentation.
    pub(super) fn shared_inputs_valid(&self) -> bool {
        self.files.values().all(|file| {
            file.declaration.parsed.physical_diagnostics().is_empty()
                && file.prepared.record.names.diagnostics().is_empty()
                && file.prepared.record.signatures.diagnostics().is_empty()
        })
    }

    /// Prepares constant prerequisites once, then projects checked imports and aggregates.
    /// Signature-only queries do not call this body-stage preparation.
    pub(super) fn body_environments(
        &self,
        const_limits: ConstLimits,
        cancel: &CancellationToken,
    ) -> Result<HashMap<FileId, BodyEnvironment>, AnalysisError> {
        cancel.check()?;
        let catalog = FunctionCatalog::new(self.files.values().map(|file| &file.prepared));
        let modules = self
            .files
            .values()
            .map(|file| (file.source().id(), &file.prepared))
            .collect();
        let aggregates = &self.aggregates;
        let mut result = HashMap::default();
        for (id, file) in self.files.iter() {
            cancel.check()?;
            let aggregates = aggregates.for_module(
                file.source().module_identity(),
                self.module_graph(),
                &self.definitions,
                cancel,
            )?;
            let mut imported_functions =
                catalog.bindings(&file.prepared.record.names.facts, cancel)?;
            imported_functions.include_inherent_methods(&aggregates);
            result.insert(
                *id,
                BodyEnvironment {
                    signatures: prepare_body_signatures(
                        file.prepared.record.signatures.facts(),
                        &file.prepared.record.declarations,
                        &aggregates,
                        cancel,
                    )?,
                    imported_statics: ImportedStatics::bindings(
                        &modules,
                        &file.prepared.record.names.facts,
                        cancel,
                    )?,
                    imported_functions,
                    imported_constants: Default::default(),
                    constants: None,
                    aggregates,
                },
            );
        }
        let mut checked = HashMap::default();
        for (id, file) in self.files.iter() {
            cancel.check()?;
            let prepared = &file.prepared;
            let environment = &result[id];
            let names = resolve_bodies(
                &prepared.lowered,
                &prepared.record.names.facts,
                BodySelection::Constants,
                cancel,
            );
            let declarations = prepared.record.declarations.clone().with_bindings(
                &prepared.lowered,
                &names,
                cancel,
            );
            let constants = check_bodies_controlled(
                &prepared.lowered,
                &names,
                &declarations,
                BodyInputs {
                    const_limits,
                    selection: BodySelection::Constants,
                    signatures: &prepared.record.signatures,
                    body_signatures: &environment.signatures,
                    imported_functions: &environment.imported_functions,
                    imported_statics: &environment.imported_statics,
                    imported_constants: &environment.imported_constants,
                    constants: None,
                    aggregates: &environment.aggregates,
                },
                cancel,
            )?;
            checked.insert(
                *id,
                Arc::new(CheckedConstants {
                    types: constants.facts.consts,
                    values: constants.facts.const_values,
                    table: constants.facts.type_table,
                    diagnostics: constants.diagnostics,
                }),
            );
        }
        for (id, environment) in &mut result {
            environment.imported_constants = ImportedConstants::bindings(
                &modules,
                &checked,
                &self.files[id].prepared.record.names.facts,
                cancel,
            )?;
            environment.constants = Some(checked[id].clone());
        }
        Ok(result)
    }

    /// Returns the declaration input snapshot revision.
    pub fn revision(&self) -> Revision {
        self.declarations.revision()
    }

    /// Finds checked signatures for an exact file ID, or `None` if absent.
    pub fn file(&self, id: FileId) -> Option<&Arc<FileSignatures>> {
        self.files.get(&id)
    }

    /// Borrows the declaration-stage inputs retained by this snapshot.
    pub fn declaration_snapshot(&self) -> &DeclarationSnapshot {
        &self.declarations
    }

    /// Borrows the declaration snapshot's resolved import graph.
    pub fn module_graph(&self) -> &ModuleGraph {
        self.declarations.module_graph()
    }
}

/// Semantic dependencies required to check one file's bodies.
#[derive(Debug)]
pub(crate) struct BodyEnvironment {
    /// Normalized local callable/static contracts shared with every body result.
    pub signatures: Arc<BodySignatures>,
    /// Callable signatures selected by the file's imports, including inherent methods.
    pub imported_functions: ImportedFunctions,
    /// Runtime static types and rebinding permissions selected by imports.
    pub imported_statics: ImportedStatics,
    /// Validated scalar values selected by source imports.
    pub imported_constants: ImportedConstants,
    /// Constant prerequisites already checked within this request.
    pub constants: Option<Arc<CheckedConstants>>,
    /// Aggregate and implementation contracts visible to this file.
    pub aggregates: AggregateCatalog,
}

impl AnalysisDatabase {
    /// Prepares declarations and checked signatures without analyzing function bodies.
    ///
    /// Builds imported type projections and aggregate contracts across the supplied sources.
    /// Ordinary source errors remain in per-file diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError`] for cancellation, invalid native inputs or identity mapping.
    pub fn signatures(
        &mut self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<SignatureSnapshot, AnalysisError> {
        let declarations = self.declarations(source, cancel)?;
        let catalog = TypeCatalog::new(declarations.files.values().map(|file| &file.declared));
        let mut imported_types = HashMap::default();
        for (id, file) in declarations.files.iter() {
            cancel.check()?;
            imported_types.insert(*id, catalog.bindings(file.names(), cancel)?);
        }
        let mut files = BTreeMap::new();
        for (id, declaration) in declarations.files.iter() {
            cancel.check()?;
            let imported = imported_types.remove(id).expect("declared type bindings");
            let prepared = declaration
                .declared
                .clone()
                .check_signatures(imported, cancel)?;
            let mut diagnostics = declaration
                .diagnostics()
                .iter()
                .cloned()
                .collect::<DiagnosticBuffer>();
            diagnostics.extend(prepared.record.signatures.diagnostics().iter().cloned());

            let metadata =
                DefinitionMetadata::checked(self.definitions.snapshot(), prepared, cancel)?;
            let definitions = metadata.definitions().clone();
            let mut prepared = metadata.into_records();
            Arc::make_mut(&mut prepared.record)
                .declarations
                .publish_definitions(definitions);
            let result = Arc::new(FileSignatures {
                declaration: declaration.clone(),
                prepared,
                diagnostics: Arc::new(diagnostics),
            });
            files.insert(*id, result);
        }
        let mut aggregates = AggregateCatalog::default();
        for file in files.values() {
            let prepared = &file.prepared;
            aggregates.add_module(
                &prepared.lowered,
                &prepared.record.declarations,
                prepared.record.signatures.facts(),
                cancel,
            )?;
        }
        for file in files.values_mut() {
            let visible = aggregates.for_module(
                file.source().module_identity(),
                &declarations.graph,
                &self.definitions.snapshot(),
                cancel,
            )?;
            let prepared = &file.prepared;
            if let Some(signatures) = prepared.completed_signatures(&visible, cancel)? {
                let signatures = ownership::scope(signatures.as_ref(), &self.definitions, cancel)?
                    .into_records();
                let file = Arc::make_mut(file);
                let record = Arc::make_mut(&mut file.prepared.record);
                record.signatures = Arc::new(signatures);
                record
                    .declarations
                    .publish_definitions(self.definitions.snapshot());
                let mut diagnostics: DiagnosticBuffer =
                    file.declaration.diagnostics().iter().cloned().collect();
                diagnostics.extend(record.signatures.diagnostics().iter().cloned());
                file.diagnostics = Arc::new(diagnostics);
            }
        }
        cancel.check()?;
        let aggregates = ownership::scope(&aggregates, &self.definitions, cancel)?.into_records();
        cancel.check()?;
        Ok(SignatureSnapshot {
            declarations,
            files: Arc::new(files),
            aggregates: Arc::new(aggregates),
            definitions: self.definitions.snapshot(),
        })
    }
}

impl FileSignatures {
    /// Borrows the table owning scoped IDs in these signature facts.
    pub fn definitions(&self) -> &DefinitionTable {
        self.prepared.record.declarations.definitions()
    }
}
