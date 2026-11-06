//! Declaration queries stop before body name resolution, typing or const evaluation.
use crate::{
    DeclaredAnalysis, DiagnosticBuffer,
    analysis::{
        AnalysisDatabase,
        error::AnalysisError,
        ownership,
        source_preparation::{self, ParsedModule},
    },
    declarations::{Declaration, DeclarationId, Declarations},
    declare_analysis,
    imports::{ModuleGraph, SourceUnit},
    native::{adoption, api as native_api},
    resolver::resolved::DeclarationNames,
};

use kagari_common::cancellation::CancellationToken;
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

/// Parsed/lowered source with module imports and declaration names, before type checking.
///
/// Owns no checked function-body facts. The lowered arena is shared with later stages;
/// [`Self::declarations`] contains declaration sites, not body-local bindings.
#[derive(Debug)]
pub struct FileDeclarations {
    pub(super) parsed: ParsedModule,
    pub(super) declared: DeclaredAnalysis,
    diagnostics: DiagnosticBuffer,
}

impl FileDeclarations {
    /// Finds a declared field or enum variant at a source byte offset.
    pub fn member_at(&self, offset: usize) -> Option<&Declaration> {
        self.declarations().member_at(offset)
    }

    /// Borrows the physical or synthetic source analyzed by this declaration result.
    pub fn source(&self) -> &SourceFile {
        &self.declared.lowered.source
    }

    /// Returns a typed AST view of the retained parse.
    pub fn syntax(&self) -> ModuleSyntax {
        self.parsed.syntax()
    }

    /// Named declarations, members and generic parameters; no local bindings.
    pub fn declarations(&self) -> &Declarations {
        &self.declared.declarations
    }

    /// Borrows module-level name/import resolution without lexical body bindings.
    pub fn names(&self) -> &DeclarationNames {
        self.declared.names.facts()
    }

    /// Parse and declaration diagnostics, excluding name/type errors in bodies.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// Immutable declarations and import graph for supplied, inline and installed sources.
///
/// `files` maps each `FileId` to a shared [`FileDeclarations`]. Inline modules have
/// synthetic file IDs while retaining their physical source origin and byte offsets.
/// Later signature queries retain this snapshot rather than independently recollecting
/// its declaration sites.
#[derive(Debug, Clone)]
pub struct DeclarationSnapshot {
    /// Returns the input source snapshot revision.
    revision: Revision,
    pub(super) graph: Arc<ModuleGraph>,
    pub(super) files: Arc<BTreeMap<FileId, Arc<FileDeclarations>>>,
}

impl DeclarationSnapshot {
    /// Returns the input source snapshot revision.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Finds declarations for an exact physical or synthetic file ID.
    pub fn file(&self, id: FileId) -> Option<&Arc<FileDeclarations>> {
        self.files.get(&id)
    }

    /// Enumerate the snapshot's user, inline-module and installed package files.
    /// Source identities and declarations remain owned by this snapshot.
    pub fn files(&self) -> impl Iterator<Item = &FileDeclarations> {
        self.files.values().map(AsRef::as_ref)
    }

    /// Source text retained by this snapshot, including authored origins of views.
    pub fn source(&self, id: FileId) -> Option<&SourceFile> {
        self.files.values().find_map(|file| {
            let source = file.source();
            if source.id() == id {
                Some(source)
            } else {
                source.copied_sources().find(|original| original.id() == id)
            }
        })
    }

    /// Borrows the resolved module graph shared by these declarations.
    pub fn module_graph(&self) -> &ModuleGraph {
        &self.graph
    }

    /// Finds a scoped identity in this retained declaration snapshot.
    pub fn declaration(&self, id: &DeclarationId) -> Option<&Declaration> {
        self.files.values().find_map(|file| {
            let id = ownership::locate(id, file.declarations().definitions())?;
            file.declarations().get(&id)
        })
    }
}

impl AnalysisDatabase {
    /// Parses/lowers sources and resolves imports and declaration names without checking bodies.
    ///
    /// Includes installed native declarations and recursively discovered inline modules.
    /// Source errors remain in each file's diagnostics. Successful preparation is published
    /// only after a cancellation check and never replaces a newer declaration snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError`] for cancellation, invalid native declaration inputs or
    /// invalid identity metadata or an import solver failure.
    pub fn declarations(
        &mut self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<DeclarationSnapshot, AnalysisError> {
        let snapshot = self.prepare_declarations(source, cancel)?;
        cancel.check()?;
        self.publish_declarations(snapshot.clone());
        Ok(snapshot)
    }

    /// Publishes a non-older declaration snapshot and prunes stale selected-body entries.
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

    /// Builds declarations using reusable parses and a newly resolved import graph.
    ///
    /// One physical parse supplies attached logical views and independent lowering
    /// arenas. Inline sources share text and line indexing with their origin.
    /// File results are retained only when source revisions, import facts and reachable
    /// namespace contents permit it. Installed declarations participate through those
    /// ordinary facts. This prepares a value; publication is separate.
    pub(super) fn prepare_declarations(
        &self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<DeclarationSnapshot, AnalysisError> {
        cancel.check()?;
        let previous = self.declaration_cache.as_ref();
        let mut lowered_files = BTreeMap::new();
        let native_files = match self.native_files.get() {
            Some(files) => files,
            None => {
                let modules = adoption::adopt(&self.native_modules, &self.definitions, cancel)?;
                let prepared = modules
                    .iter()
                    .enumerate()
                    .map(|(index, module)| match &self.native_sources {
                        Some(sources) => native_api::import_source(
                            module,
                            &modules,
                            &sources[index],
                            &self.source_root,
                            self.parse_limits,
                            cancel,
                        ),
                        None => native_api::import(
                            module,
                            &modules,
                            &self.source_root,
                            self.parse_limits,
                            cancel,
                        ),
                    })
                    .collect::<Result<Vec<_>, _>>();
                cancel.check()?;
                self.native_files
                    .set(prepared?)
                    .expect("single native analysis owner");
                self.native_files.get().expect("installed native API files")
            }
        };
        for (parsed, lowered) in native_files {
            lowered_files.insert(
                lowered.source.id(),
                (ParsedModule::root(parsed.clone()), lowered.clone()),
            );
        }
        for (id, prepared) in source_preparation::prepare(self, &source, previous, cancel)? {
            lowered_files.insert(id, (prepared.parsed, prepared.lowered));
        }
        #[cfg(test)]
        let limits = self.import_solver_limits;
        #[cfg(not(test))]
        let limits = None;
        let graph = Arc::new(ModuleGraph::build_with_limits(
            lowered_files.values().map(|(_, lowered)| lowered.as_ref()),
            &self.definitions,
            source.packages(),
            cancel,
            limits,
        )?);
        let mut files = BTreeMap::new();
        for (id, (parsed, lowered)) in lowered_files {
            cancel.check()?;
            let imports = graph
                .imports_for(&SourceUnit::of(&lowered, &graph.catalog.definitions))
                .expect("declared module is in graph")
                .clone();
            let old = previous
                .and_then(|snapshot| snapshot.file(id))
                .filter(|old| {
                    old.source().revision() == lowered.source.revision()
                        && old.names().imports == imports
                        && old
                            .names()
                            .catalog
                            .same_namespaces(&graph.catalog, &imports.scope, cancel)
                            .unwrap_or(false)
                });
            let file = if let Some(old) = old {
                old.clone()
            } else {
                let declared = declare_analysis(
                    lowered,
                    imports,
                    graph.catalog.clone(),
                    &self.definitions,
                    cancel,
                )?;
                let mut diagnostics = declared.names.diagnostics.clone();
                diagnostics.extend(parsed.diagnostics().cloned());
                let metadata = ownership::scope(&declared, &self.definitions, cancel)?;
                let definitions = metadata.definitions().clone();
                let mut declared = metadata.into_records();
                declared.declarations.publish_definitions(definitions);
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
            graph,
            files: Arc::new(files),
        })
    }
}
