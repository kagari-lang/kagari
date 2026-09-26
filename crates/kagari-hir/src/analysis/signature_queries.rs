//! Signature queries consume declarations without running any body analysis.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct FileSignatures {
    pub(super) declaration: Arc<FileDeclarations>,
    pub(super) prepared: crate::PreparedAnalysis,
    diagnostics: crate::DiagnosticBuffer,
}

impl FileSignatures {
    /// Navigate checked signature type names and declaration sites without
    /// resolving any function body.
    pub fn definition_at(&self, offset: usize) -> Option<&crate::declarations::Declaration> {
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
    pub fn host_type_at(
        &self,
        offset: usize,
    ) -> Option<&kagari_common::host_interface::HostTypeDeclaration> {
        match type_reference_target_at(
            &self.prepared.lowered,
            self.prepared.signatures.facts().type_table(),
            offset,
        )? {
            Some(crate::typeck::TypeTarget::Host(id)) => {
                self.prepared.declarations.hosts.type_declaration(id)
            }
            _ => None,
        }
    }

    /// Type annotations checked by this signature query; body annotations have
    /// no facts until a body query checks them.
    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        type_at_in(
            &self.prepared.lowered,
            self.prepared.signatures.facts().type_table(),
            offset,
        )
    }

    pub fn source(&self) -> &SourceFile {
        self.declaration.source()
    }
    pub fn declarations(&self) -> &crate::declarations::Declarations {
        &self.prepared.declarations
    }
    pub fn signatures(&self) -> &Arc<AnalysisResult<crate::typeck::ModuleSignatures>> {
        &self.prepared.signatures
    }
    pub fn diagnostics(&self) -> &[kagari_common::Diagnostic] {
        &self.diagnostics
    }
    /// Whether construction reused checked signature facts from an earlier query.
    pub fn reused(&self) -> bool {
        self.prepared.signatures_reused
    }
}

#[derive(Debug, Clone)]
pub struct SignatureSnapshot {
    pub(super) declarations: DeclarationSnapshot,
    pub(super) files: Arc<std::collections::BTreeMap<FileId, Arc<FileSignatures>>>,
    aggregates: Arc<crate::aggregates::AggregateCatalog>,
}

impl SignatureSnapshot {
    pub(super) fn body_environments(
        &self,
        cancel: &CancellationToken,
    ) -> Result<HashMap<FileId, BodyEnvironment>, Cancelled> {
        let catalog =
            crate::imports::FunctionCatalog::new(self.files.values().map(|file| &file.prepared));
        let mut result = HashMap::new();
        for (id, file) in self.files.iter() {
            cancel.check()?;
            let aggregates = self.aggregates.for_module(
                file.source().module_identity(),
                self.module_graph(),
                cancel,
            )?;
            let mut imported_functions =
                catalog.bindings(&file.prepared.names.facts.imports, cancel)?;
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

    pub fn revision(&self) -> Revision {
        self.declarations.revision()
    }
    pub fn host_revision(&self) -> u64 {
        self.declarations.host_revision()
    }
    pub fn file(&self, id: FileId) -> Option<&Arc<FileSignatures>> {
        self.files.get(&id)
    }
    pub fn declaration_snapshot(&self) -> &DeclarationSnapshot {
        &self.declarations
    }
    pub fn module_graph(&self) -> &crate::imports::ModuleGraph {
        self.declarations.module_graph()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BodyEnvironment {
    pub imported_functions: crate::imports::ImportedFunctions,
    pub aggregates: crate::aggregates::AggregateCatalog,
}

impl AnalysisDatabase {
    pub fn signatures(
        &mut self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<SignatureSnapshot, Cancelled> {
        let snapshot = self.prepare_signatures(source, cancel)?;
        cancel.check()?;
        self.publish_signatures(snapshot.clone());
        Ok(snapshot)
    }

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

    pub(super) fn prepare_signatures(
        &self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<SignatureSnapshot, Cancelled> {
        let declarations = self.prepare_declarations(source, cancel)?;
        let catalog = crate::imports::TypeCatalog::new(
            declarations.files.values().map(|file| &file.declared),
        );
        let mut imported_types = HashMap::new();
        for (id, file) in declarations.files.iter() {
            cancel.check()?;
            imported_types.insert(*id, catalog.bindings(&file.names().imports, cancel)?);
        }
        let mut files = std::collections::BTreeMap::new();
        for (id, declaration) in declarations.files.iter() {
            cancel.check()?;
            let imported = imported_types.remove(id).expect("declared type bindings");
            let old = self.signature_cache.as_ref().and_then(|old| old.file(*id));
            let result = if let Some(old) = old.filter(|old| {
                Arc::ptr_eq(&old.declaration, declaration)
                    && old.prepared.declarations.imported_types == imported
            }) {
                old.clone()
            } else {
                let prepared = declaration.declared.clone().check_signatures(
                    imported,
                    old.map(|old| &old.prepared),
                    cancel,
                );
                let mut diagnostics = declaration
                    .diagnostics()
                    .iter()
                    .cloned()
                    .collect::<crate::DiagnosticBuffer>();
                diagnostics.extend(prepared.signatures.diagnostics().iter().cloned());
                Arc::new(FileSignatures {
                    declaration: declaration.clone(),
                    prepared,
                    diagnostics,
                })
            };
            files.insert(*id, result);
        }
        let mut aggregates = crate::aggregates::AggregateCatalog::default();
        for file in files.values() {
            let prepared = &file.prepared;
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
            if let Some(signatures) = file.prepared.completed_signatures(&visible, cancel)? {
                let file = Arc::make_mut(file);
                file.prepared.signatures = signatures;
                file.diagnostics = file.declaration.diagnostics().iter().cloned().collect();
                file.diagnostics
                    .extend(file.prepared.signatures.diagnostics().iter().cloned());
            }
        }
        cancel.check()?;
        Ok(SignatureSnapshot {
            declarations,
            files: Arc::new(files),
            aggregates: Arc::new(aggregates),
        })
    }
}
