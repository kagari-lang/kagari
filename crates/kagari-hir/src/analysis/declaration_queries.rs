//! Declaration queries stop before body name resolution, typing or const evaluation.
use super::*;
use crate::{
    DeclaredAnalysis, DiagnosticBuffer,
    declarations::{Declaration, DeclarationId, Declarations},
};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct FileDeclarations {
    pub(super) parsed: kagari_syntax::Parse,
    pub(super) declared: DeclaredAnalysis,
    diagnostics: DiagnosticBuffer,
}

impl FileDeclarations {
    pub fn source(&self) -> &SourceFile {
        &self.declared.lowered.source
    }

    pub fn syntax(&self) -> kagari_syntax::ast::SourceFile {
        self.parsed.syntax()
    }

    /// Named declarations, fields and generic parameters; no local bindings.
    pub fn declarations(&self) -> &Declarations {
        &self.declared.declarations
    }

    pub fn names(&self) -> &crate::resolver::DeclarationNames {
        self.declared.names.facts()
    }

    /// Parse and declaration diagnostics, excluding name/type errors in bodies.
    pub fn diagnostics(&self) -> &[kagari_common::Diagnostic] {
        &self.diagnostics
    }
}

#[derive(Debug, Clone)]
pub struct DeclarationSnapshot {
    revision: Revision,
    host_revision: u64,
    pub(super) graph: Arc<crate::imports::ModuleGraph>,
    pub(super) files: Arc<std::collections::BTreeMap<FileId, Arc<FileDeclarations>>>,
}

impl DeclarationSnapshot {
    pub fn revision(&self) -> Revision {
        self.revision
    }
    pub fn host_revision(&self) -> u64 {
        self.host_revision
    }
    pub fn file(&self, id: FileId) -> Option<&Arc<FileDeclarations>> {
        self.files.get(&id)
    }
    pub fn module_graph(&self) -> &crate::imports::ModuleGraph {
        &self.graph
    }
    pub fn declaration(&self, id: &DeclarationId) -> Option<&Declaration> {
        self.files
            .values()
            .find_map(|file| file.declarations().get(id))
    }
}

impl AnalysisDatabase {
    pub fn declarations(
        &mut self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<DeclarationSnapshot, Cancelled> {
        let snapshot = self.prepare_declarations(source, cancel)?;
        cancel.check()?;
        self.publish_declarations(snapshot.clone());
        Ok(snapshot)
    }

    pub(super) fn publish_declarations(&mut self, snapshot: DeclarationSnapshot) {
        self.publish_body(&snapshot, None);
        if self
            .declaration_cache
            .as_ref()
            .is_none_or(|old| snapshot.revision >= old.revision)
        {
            self.declaration_cache = Some(snapshot);
        }
    }

    pub(super) fn prepare_declarations(
        &self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<DeclarationSnapshot, Cancelled> {
        cancel.check()?;
        let previous = self.declaration_cache.as_ref();
        let mut lowered_files = std::collections::BTreeMap::new();
        for file in source.files() {
            cancel.check()?;
            let old = previous
                .and_then(|snapshot| snapshot.file(file.id()))
                .filter(|old| old.source().revision() == file.revision());
            let (parsed, lowered) = match old {
                Some(old) => (old.parsed.clone(), old.declared.lowered.clone()),
                None => {
                    let parsed = kagari_syntax::parser::parse_with_cancellation(file, cancel)?;
                    let lowered = crate::lower::lower_module_controlled(
                        file.clone(),
                        &parsed.syntax(),
                        cancel,
                    );
                    (parsed, Arc::new(lowered))
                }
            };
            lowered_files.insert(file.id(), (parsed, lowered));
        }
        let graph = Arc::new(crate::imports::ModuleGraph::build(
            lowered_files.values().map(|(_, lowered)| lowered.as_ref()),
            &self.hosts,
            cancel,
        )?);
        let mut files = std::collections::BTreeMap::new();
        for (id, (parsed, lowered)) in lowered_files {
            cancel.check()?;
            let imports = graph
                .node(lowered.source.module_identity())
                .expect("declared module is in graph")
                .imports
                .clone();
            let old = previous
                .and_then(|snapshot| snapshot.file(id))
                .filter(|old| {
                    old.source().revision() == lowered.source.revision()
                        && old.names().hosts.revision() == self.hosts.revision()
                        && old.names().imports == imports
                });
            let file = if let Some(old) = old {
                old.clone()
            } else {
                let declared =
                    crate::declare_analysis(lowered, self.hosts.clone(), imports, cancel);
                let mut diagnostics = declared.names.diagnostics.clone();
                diagnostics.extend(parsed.diagnostics().iter().cloned());
                Arc::new(FileDeclarations {
                    parsed,
                    declared,
                    diagnostics,
                })
            };
            files.insert(id, file);
        }
        cancel.check()?;
        Ok(DeclarationSnapshot {
            revision: source.revision(),
            host_revision: self.hosts.revision(),
            graph,
            files: Arc::new(files),
        })
    }
}
