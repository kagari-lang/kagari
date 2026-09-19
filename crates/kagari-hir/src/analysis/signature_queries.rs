//! Signature queries consume declarations without running any body analysis.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct FileSignatures {
    pub(super) declaration: Arc<FileDeclarations>,
    pub(super) prepared: crate::PreparedAnalysis,
    diagnostics: crate::DiagnosticBuffer,
}

impl FileSignatures {
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
}

impl SignatureSnapshot {
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
            &declarations.graph,
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
        cancel.check()?;
        Ok(SignatureSnapshot {
            declarations,
            files: Arc::new(files),
        })
    }
}
