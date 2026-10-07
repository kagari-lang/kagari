//! Signature queries consume declarations without running any body analysis.

use crate::{
    AnalysisResult, DiagnosticBuffer, PreparedAnalysis,
    aggregates::AggregateCatalog,
    analysis::{
        AnalysisDatabase,
        declaration_queries::{DeclarationSnapshot, FileDeclarations},
        error::AnalysisError,
        ownership, type_at_in, type_reference_at, type_reference_target_at,
    },
    declarations::{Declaration, Declarations},
    imports::{
        ModuleGraph,
        functions::{FunctionCatalog, ImportedFunctions},
        types::TypeCatalog,
    },
    typeck::{ModuleSignatures, table::TypeTarget},
    types::TypeId,
};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath,
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
        reference::DefinitionReference,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_source::{
    diagnostic::Diagnostic,
    identity::{FileId, Revision},
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_types::host_interface::type_declaration::HostTypeDeclaration;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

#[cfg(test)]
mod tests;

/// One file's declaration snapshot plus checked signature facts and accumulated diagnostics.
#[derive(Debug, Clone)]
pub struct FileSignatures {
    pub(super) declaration: Arc<FileDeclarations>,
    pub(super) prepared: PreparedAnalysis<DefinitionId>,
    /// Returns accumulated diagnostics through the signature stage, excluding function bodies.
    diagnostics: DiagnosticBuffer,
}

impl FileSignatures {
    /// Navigate checked signature type names and declaration sites without
    /// resolving any function body.
    pub fn definition_at(&self, offset: usize) -> Option<&Declaration<DefinitionId>> {
        self.prepared.declarations.site_at(offset).or_else(|| {
            type_reference_at(
                &self.prepared.lowered,
                self.prepared.signatures.facts().type_table(),
                &self.prepared.declarations,
                offset,
            )
            .flatten()
        })
    }

    /// Read an offline host type declaration from a checked signature name.
    pub fn host_type_at(&self, offset: usize) -> Option<&HostTypeDeclaration> {
        match type_reference_target_at(
            &self.prepared.lowered,
            self.prepared.signatures.facts().type_table(),
            offset,
        )? {
            Some(TypeTarget::Host(id)) => self.prepared.declarations.hosts.type_declaration(id),
            _ => None,
        }
    }

    /// Type annotations checked by this signature query; body annotations have
    /// no facts until a body query checks them.
    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        let ty = type_at_in(
            &self.prepared.lowered,
            self.prepared.signatures.facts().type_table(),
            offset,
        )?;
        ownership::paths(
            &ty,
            self.declarations().definitions(),
            &CancellationToken::default(),
        )
        .ok()
    }

    /// Borrows the source retained by the declaration stage.
    pub fn source(&self) -> &SourceFile {
        self.declaration.source()
    }

    /// Borrows declaration identities and imported type bindings used by signature checking.
    pub fn declarations(&self) -> &Declarations<DefinitionId> {
        &self.prepared.declarations
    }

    /// Borrows checked signatures and their recoverable signature diagnostics.
    pub fn signatures(&self) -> &Arc<AnalysisResult<ModuleSignatures<DefinitionId>>> {
        &self.prepared.signatures
    }

    /// Returns accumulated diagnostics through the signature stage, excluding function bodies.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Whether construction reused checked signature facts from an earlier query.
    pub fn reused(&self) -> bool {
        self.prepared.signatures_reused
    }
}

/// Checked signatures for all declaration files, before function-body checking.
///
/// Retains a [`DeclarationSnapshot`], a `FileId -> Arc<FileSignatures>` map and a
/// shared aggregate catalog. Body queries derive visible function/aggregate inputs
/// from this snapshot. Reuse depends on declaration results and imported type inputs;
/// completing aggregate contracts can add signature diagnostics.
#[derive(Debug, Clone)]
pub struct SignatureSnapshot {
    pub(super) declarations: DeclarationSnapshot,
    pub(super) files: Arc<BTreeMap<FileId, Arc<FileSignatures>>>,
    aggregates: Arc<AggregateCatalog<DefinitionId>>,
    definitions: DefinitionTable,
}

impl SignatureSnapshot {
    /// Projects signatures into per-file imported callable and visible aggregate inputs.
    pub(super) fn body_environments(
        &self,
        cancel: &CancellationToken,
    ) -> Result<HashMap<FileId, BodyEnvironment>, AnalysisError> {
        let prepared = self
            .files
            .values()
            .map(|file| file.authoring(cancel))
            .collect::<Result<Vec<_>, _>>()?;
        let aggregates = ownership::paths(self.aggregates.as_ref(), &self.definitions, cancel)?;
        let catalog = FunctionCatalog::new(prepared.iter());
        let mut result = HashMap::new();
        for (id, file) in self.files.iter() {
            cancel.check()?;
            let aggregates = aggregates.for_module(
                file.source().module_identity(),
                self.module_graph(),
                cancel,
            )?;
            let mut imported_functions = catalog.bindings(&file.prepared.names.facts, cancel)?;
            imported_functions.include_inherent_methods(&aggregates);
            result.insert(
                *id,
                BodyEnvironment {
                    imported_functions,
                    aggregates,
                },
            );
        }
        Ok(result)
    }

    /// Returns the declaration input snapshot revision.
    pub fn revision(&self) -> Revision {
        self.declarations.revision()
    }

    /// Returns the host registry revision inherited from declaration analysis.
    pub fn host_revision(&self) -> u64 {
        self.declarations.host_revision()
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
#[derive(Debug, Clone, PartialEq)]
pub(super) struct BodyEnvironment<I: DefinitionReference = DefinitionPath> {
    /// Callable signatures selected by the file's imports, including inherent methods.
    pub imported_functions: ImportedFunctions<I>,
    /// Aggregate and implementation contracts visible to this file.
    pub aggregates: AggregateCatalog<I>,
}

impl AnalysisDatabase {
    /// Prepares declarations and checked signatures without analyzing function bodies.
    ///
    /// Builds imported type projections and aggregate contracts across the supplied sources.
    /// Ordinary source errors remain in per-file diagnostics; successful preparation is
    /// published after checking cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError`] for cancellation, invalid native inputs or identity mapping.
    pub fn signatures(
        &mut self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<SignatureSnapshot, AnalysisError> {
        let snapshot = self.prepare_signatures(source, cancel)?;
        cancel.check()?;
        self.publish_signatures(snapshot.clone());
        Ok(snapshot)
    }

    /// Publishes the declaration dependency and signatures only when their revisions are not older.
    pub(super) fn publish_signatures(&mut self, snapshot: SignatureSnapshot) {
        self.publish_declarations(snapshot.declarations.clone());
        if self
            .signature_cache
            .as_ref()
            .is_none_or(|old| snapshot.revision() >= old.revision())
        {
            self.signature_cache = Some(snapshot);
        }
    }

    /// Checks or reuses file signatures, then completes visible aggregate contracts before publication.
    pub(super) fn prepare_signatures(
        &self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<SignatureSnapshot, AnalysisError> {
        let declarations = self.prepare_declarations(source, cancel)?;
        let authoring_declarations = declarations
            .files
            .iter()
            .map(|(id, file)| Ok((*id, file.authoring(cancel)?)))
            .collect::<Result<BTreeMap<_, _>, AnalysisError>>()?;
        let catalog = TypeCatalog::new(authoring_declarations.values());
        let mut imported_types = HashMap::new();
        for (id, file) in declarations.files.iter() {
            cancel.check()?;
            imported_types.insert(*id, catalog.bindings(file.names(), cancel)?);
        }
        let mut files = BTreeMap::new();
        for (id, declaration) in declarations.files.iter() {
            cancel.check()?;
            let imported = imported_types.remove(id).expect("declared type bindings");
            let old = self.signature_cache.as_ref().and_then(|old| old.file(*id));
            let previous = old.map(|old| old.authoring(cancel)).transpose()?;
            let result = if let Some(old) = old.filter(|old| {
                Arc::ptr_eq(&old.declaration, declaration)
                    && previous
                        .as_ref()
                        .expect("old authoring signatures")
                        .declarations
                        .imported_types
                        == imported
            }) {
                old.clone()
            } else {
                let prepared = authoring_declarations[id].clone().check_signatures(
                    imported,
                    previous.as_ref(),
                    cancel,
                );
                let mut diagnostics = declaration
                    .diagnostics()
                    .iter()
                    .cloned()
                    .collect::<DiagnosticBuffer>();
                diagnostics.extend(prepared.signatures.diagnostics().iter().cloned());
                let shared_signatures = previous
                    .as_ref()
                    .zip(old)
                    .filter(|(previous, _)| Arc::ptr_eq(&prepared.signatures, &previous.signatures))
                    .map(|(_, old)| old.prepared.signatures.clone());
                let metadata = ownership::scope(&prepared, &self.definitions, cancel)?;
                let definitions = metadata.definitions().clone();
                let mut prepared = metadata.into_records();
                prepared.declarations.publish_definitions(definitions);
                if let Some(signatures) = shared_signatures {
                    prepared.signatures = signatures;
                }
                Arc::new(FileSignatures {
                    declaration: declaration.clone(),
                    prepared,
                    diagnostics,
                })
            };
            files.insert(*id, result);
        }
        let mut aggregates = AggregateCatalog::default();
        for file in files.values() {
            let prepared = file.authoring(cancel)?;
            aggregates.add_module(
                &prepared.lowered,
                &prepared.declarations,
                prepared.signatures.facts(),
                cancel,
            )?;
        }
        for file in files.values_mut() {
            let visible = aggregates.for_module(
                file.source().module_identity(),
                &declarations.graph,
                cancel,
            )?;
            let authoring = file.authoring(cancel)?;
            if let Some(signatures) = authoring.completed_signatures(&visible, cancel)? {
                let signatures = ownership::scope(signatures.as_ref(), &self.definitions, cancel)?
                    .into_records();
                let file = Arc::make_mut(file);
                file.prepared.signatures = Arc::new(signatures);
                file.prepared
                    .declarations
                    .publish_definitions(self.definitions.snapshot());
                file.diagnostics = file.declaration.diagnostics().iter().cloned().collect();
                file.diagnostics
                    .extend(file.prepared.signatures.diagnostics().iter().cloned());
            }
        }
        cancel.check()?;
        let aggregates = ownership::scope(&aggregates, &self.definitions, cancel)?.into_records();
        Ok(SignatureSnapshot {
            declarations,
            files: Arc::new(files),
            aggregates: Arc::new(aggregates),
            definitions: self.definitions.snapshot(),
        })
    }
}

impl FileSignatures {
    pub(super) fn authoring(
        &self,
        cancel: &CancellationToken,
    ) -> Result<PreparedAnalysis, AnalysisError> {
        Ok(ownership::paths(
            &self.prepared,
            self.declarations().definitions(),
            cancel,
        )?)
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for BodyEnvironment<I> {
    type Rebind<J: DefinitionReference> = BodyEnvironment<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        Ok(BodyEnvironment {
            imported_functions: self.imported_functions.map_identities(mapper)?,
            aggregates: self.aggregates.map_identities(mapper)?,
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.imported_functions.visit_definitions(visit, cancel)?;
        self.aggregates.visit_definitions(visit, cancel)
    }
}

impl FileSignatures {
    /// Borrows the table owning scoped IDs in these signature facts.
    pub fn definitions(&self) -> &DefinitionTable {
        self.prepared.declarations.definitions()
    }
}
