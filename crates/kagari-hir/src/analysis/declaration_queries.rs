//! Declaration queries stop before body name resolution, typing or const evaluation.
use crate::{
    DeclaredAnalysis, DiagnosticBuffer,
    analysis::{AnalysisDatabase, error::AnalysisError, ownership},
    declarations::{Declaration, DeclarationId, Declarations},
    declare_analysis,
    imports::{ModuleGraph, SourceUnit},
    lower,
    native::api as native_api,
    resolver::resolved::DeclarationNames,
};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{reference::DefinitionReference, table::DefinitionId},
    span::Span,
};
use kagari_source::{
    diagnostic::Diagnostic,
    identity::{FileId, Revision},
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_syntax::{
    ast::{
        item::{Item, SourceFile as AstSourceFile},
        traits::AstNode,
    },
    parser,
    parser::Parse,
};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    sync::Arc,
};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct FileDeclarations {
    pub(super) parsed: Parse,
    pub(super) declared: DeclaredAnalysis<DefinitionId>,
    diagnostics: DiagnosticBuffer,
}

impl FileDeclarations {
    pub fn member_at(&self, offset: usize) -> Option<&Declaration<DefinitionId>> {
        self.declarations().member_at(offset)
    }

    pub fn source(&self) -> &SourceFile {
        &self.declared.lowered.source
    }

    pub fn syntax(&self) -> AstSourceFile {
        self.parsed.syntax()
    }

    /// Named declarations, members and generic parameters; no local bindings.
    pub fn declarations(&self) -> &Declarations<DefinitionId> {
        &self.declared.declarations
    }

    pub fn names(&self) -> &DeclarationNames {
        self.declared.names.facts()
    }

    /// Parse and declaration diagnostics, excluding name/type errors in bodies.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

#[derive(Debug, Clone)]
pub struct DeclarationSnapshot {
    revision: Revision,
    host_revision: u64,
    pub(super) graph: Arc<ModuleGraph>,
    pub(super) files: Arc<BTreeMap<FileId, Arc<FileDeclarations>>>,
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

    pub fn module_graph(&self) -> &ModuleGraph {
        &self.graph
    }

    pub fn declaration<I: DefinitionReference>(
        &self,
        id: &DeclarationId<I>,
    ) -> Option<&Declaration<DefinitionId>> {
        self.files.values().find_map(|file| {
            let id = ownership::locate(id, file.declarations().definitions())?;
            file.declarations().get(&id)
        })
    }
}

impl AnalysisDatabase {
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
    ) -> Result<DeclarationSnapshot, AnalysisError> {
        cancel.check()?;
        let previous = self.declaration_cache.as_ref();
        let mut lowered_files = BTreeMap::new();
        let native_files = match self.native_files.get() {
            Some(files) => files,
            None => {
                let modules = &self.native_modules;
                let prepared = modules
                    .iter()
                    .enumerate()
                    .map(|(index, module)| match &self.native_sources {
                        Some(sources) => native_api::import_source(
                            module,
                            modules,
                            &sources[index],
                            self.parse_limits,
                            cancel,
                        ),
                        None => native_api::import(module, modules, self.parse_limits, cancel),
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
            lowered_files.insert(lowered.source.id(), (parsed.clone(), lowered.clone()));
        }
        let mut pending = VecDeque::from_iter(source.files().cloned());
        let mut generated = HashSet::new();
        while let Some(file) = pending.pop_front() {
            cancel.check()?;
            let old = previous
                .and_then(|snapshot| snapshot.file(file.id()))
                .filter(|old| old.source().revision() == file.revision());
            let (parsed, lowered) = match old {
                Some(old) => (old.parsed.clone(), old.declared.lowered.clone()),
                None => {
                    let parsed = parser::parse_with_limits(&file, self.parse_limits, cancel)?;
                    let lowered =
                        lower::lower_module_controlled(file.clone(), &parsed.syntax(), cancel);
                    (parsed, Arc::new(lowered))
                }
            };
            for item in parsed.syntax().items() {
                let Item::ModuleDef(module) = item else {
                    continue;
                };
                let (Some(name), Some(block)) = (module.name_text(), module.block()) else {
                    continue;
                };
                if !generated.insert((file.id(), name.clone())) {
                    continue;
                }
                let range = block.syntax().text_range();
                let start = usize::from(range.start()) + 1;
                let end = usize::from(range.end()).saturating_sub(1);
                let mut bytes = file.text().as_bytes().to_vec();
                for (index, byte) in bytes.iter_mut().enumerate() {
                    if !(start..end).contains(&index) && !matches!(*byte, b'\r' | b'\n') {
                        *byte = b' ';
                    }
                }
                let text = String::from_utf8(bytes).expect("inline module spans preserve UTF-8");
                let id = *self
                    .inline_ids
                    .borrow_mut()
                    .entry((file.id(), name.clone()))
                    .or_insert_with(|| SourceFile::new(file.name(), "").id());
                pending.push_back(Arc::new(SourceFile::inline_module(
                    &file,
                    &name,
                    text,
                    id,
                    Span::new(start, end),
                )));
            }
            lowered_files.insert(file.id(), (parsed, lowered));
        }
        let graph = Arc::new(ModuleGraph::build(
            lowered_files.values().map(|(_, lowered)| lowered.as_ref()),
            &self.hosts,
            cancel,
        )?);
        let mut files = BTreeMap::new();
        for (id, (parsed, lowered)) in lowered_files {
            cancel.check()?;
            let imports = graph
                .imports_for(&SourceUnit::of(&lowered))
                .expect("declared module is in graph")
                .clone();
            let old = previous
                .and_then(|snapshot| snapshot.file(id))
                .filter(|old| {
                    old.source().revision() == lowered.source.revision()
                        && old.names().hosts.revision() == self.hosts.revision()
                        && old.names().imports == imports
                        && old
                            .names()
                            .catalog
                            .same_namespaces(&graph.catalog, &imports.scope, &self.hosts, cancel)
                            .unwrap_or(false)
                });
            let file = if let Some(old) = old {
                old.clone()
            } else {
                let declared = declare_analysis(
                    lowered,
                    self.hosts.clone(),
                    imports,
                    graph.catalog.clone(),
                    &self.definitions,
                    cancel,
                );
                let mut diagnostics = declared.names.diagnostics.clone();
                diagnostics.extend(parsed.diagnostics().iter().cloned());
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
            host_revision: self.hosts.revision(),
            graph,
            files: Arc::new(files),
        })
    }
}

impl FileDeclarations {
    pub(super) fn authoring(
        &self,
        cancel: &CancellationToken,
    ) -> Result<DeclaredAnalysis, AnalysisError> {
        Ok(ownership::paths(
            &self.declared,
            self.declarations().definitions(),
            cancel,
        )?)
    }
}
