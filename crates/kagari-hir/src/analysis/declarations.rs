//! Declaration queries stop before body name resolution, typing or const evaluation.
use crate::{
    DeclaredAnalysis, DiagnosticBuffer,
    analysis::{
        AnalysisDatabase,
        error::AnalysisError,
        ownership,
        source_preparation::{self, ParsedModule, PreparedSource, SourcePreparer},
    },
    declarations::{Declaration, DeclarationId, Declarations},
    declare_analysis,
    imports::{ModuleGraph, SourceUnit},
    lower::LoweredModule,
    native::{adoption, api::preparation::NativeFiles, paths::LibraryPaths},
    resolver::resolved::DeclarationNames,
};

use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    identity::{FileId, Revision},
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_syntax::ast::module::ModuleSyntax;
use kagari_types::declaration::module::{DeclarationError, ModuleDecl};
use std::{collections::BTreeMap, sync::Arc};

#[cfg(test)]
mod tests;

/// Check attribute names before publishing header diagnostics; `lang` has its own validator.
pub(crate) fn validate_attribute_names(
    lowered: &LoweredModule,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) -> Result<(), AnalysisError> {
    for attribute in &lowered.module.attributes {
        cancel.check()?;
        let kind = match attribute.name.as_str() {
            "meta" | "lang" => continue,
            "reflect" | "requires" | "profile" => DiagnosticKind::UnsupportedAttribute {
                name: attribute.name.clone(),
            },
            name if name.starts_with("tool::") => continue,
            _ => DiagnosticKind::UnknownAttribute {
                name: attribute.name.clone(),
            },
        };
        diagnostics.push(Diagnostic::error(kind).with_span(attribute.span));
    }
    Ok(())
}

/// Parsed/lowered source with module imports and declaration names, before type checking.
///
/// Owns no checked function-body facts. The lowered arena is shared with later stages;
/// [`Self::declarations`] contains declaration sites, not body-local bindings.
#[derive(Debug)]
pub struct FileDeclarations {
    pub(super) parsed: ParsedModule,
    pub(super) declared: DeclaredAnalysis,
    diagnostics: Arc<DiagnosticBuffer>,
}

impl FileDeclarations {
    /// Borrows the physical or synthetic source analyzed by this declaration result.
    pub fn source(&self) -> &SourceFile {
        &self.declared.lowered.source
    }

    /// Returns the retained source AST, or `None` for a structured declaration input.
    pub fn syntax(&self) -> Option<ModuleSyntax> {
        self.parsed.syntax()
    }

    /// Borrows the structural lowering and origins matching these declarations.
    pub fn lowered(&self) -> &LoweredModule {
        &self.declared.lowered
    }

    /// Named declarations, members and generic parameters; no local bindings.
    pub fn declarations(&self) -> &Declarations {
        &self.declared.record.declarations
    }

    /// Borrows module-level name/import resolution without lexical body bindings.
    pub fn names(&self) -> &DeclarationNames {
        self.declared.record.names.facts()
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
    native_modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    /// Returns the input source snapshot revision.
    revision: Revision,
    pub(super) graph: Arc<ModuleGraph>,
    pub(super) files: Arc<BTreeMap<FileId, Arc<FileDeclarations>>>,
}

impl DeclarationSnapshot {
    /// Retained registration metadata in its provider scopes, including documentation.
    /// This supplies no authored source or navigation coordinates.
    pub fn native_modules(&self) -> &[Arc<DefinitionMetadata<ModuleDecl>>] {
        &self.native_modules
    }

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
    /// Source errors remain in each file's diagnostics. The returned snapshot pairs
    /// each logical lowering with its parse and resolves names using the current inputs.
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
        cancel.check()?;
        let mut lowered_files = self.prepare_core(cancel)?;
        let paths = Arc::new(
            LibraryPaths::new(&self.native_modules, &self.core).with_importers(
                &self.native_modules,
                &self.import_environment,
                source.packages(),
            ),
        );
        if self
            .native_files
            .get()
            .is_some_and(|files| !files.matches_paths(&paths))
        {
            self.native_files.take();
        }
        let native_files = match self.native_files.get() {
            Some(files) => {
                let attached = files.attach(cancel);
                cancel.check()?;
                attached?
            }
            None => {
                let modules = adoption::adopt(&self.native_modules, &self.definitions, cancel)?;
                let prepared = NativeFiles::new(modules, paths.clone(), &self.source_root, cancel);
                cancel.check()?;
                let prepared = prepared?;
                let attached = prepared.attach(cancel);
                cancel.check()?;
                let attached = attached?;
                self.native_files
                    .set(prepared)
                    .expect("single native analysis owner");
                attached
            }
        };
        for lowered in native_files {
            if lowered_files.values().any(|file| {
                file.lowered.source.module_identity() == lowered.source.module_identity()
            }) {
                return Err(DeclarationError(
                    "bundled module conflicts with registered provider".into(),
                )
                .into());
            }
            if lowered_files
                .insert(
                    lowered.source.id(),
                    PreparedSource {
                        parsed: ParsedModule::structured(),
                        lowered,
                    },
                )
                .is_some()
            {
                return Err(DeclarationError(
                    "bundled file conflicts with registered provider".into(),
                )
                .into());
            }
        }
        let preparer = SourcePreparer {
            inline_ids: &mut self.inline_ids,
            limits: self.parse_limits,
        };
        let project = source_preparation::prepare(preparer, &source, cancel)?;
        // Competing library owners are input errors; two project files claiming one
        // module must both reach the import solver so it can diagnose ambiguity.
        for (id, prepared) in &project {
            cancel.check()?;
            if lowered_files.contains_key(id)
                || lowered_files.values().any(|library| {
                    library.lowered.source.module_identity()
                        == prepared.lowered.source.module_identity()
                })
            {
                return Err(DeclarationError(
                    "source conflicts with library declaration input".into(),
                )
                .into());
            }
        }
        lowered_files.extend(project);
        #[cfg(test)]
        let limits = self.import_solver_limits;
        #[cfg(not(test))]
        let limits = None;
        let environment = self.import_scopes(
            lowered_files.values().map(|file| file.lowered.as_ref()),
            source.packages(),
            &paths,
            cancel,
        )?;
        let graph = Arc::new(ModuleGraph::build_with_environment(
            lowered_files.values().map(|file| file.lowered.as_ref()),
            &self.definitions,
            source.packages(),
            &environment,
            cancel,
            limits,
        )?);
        let mut files = BTreeMap::new();
        for (id, PreparedSource { parsed, lowered }) in lowered_files {
            cancel.check()?;
            let imports = graph
                .imports_for(&SourceUnit::of(&lowered, &graph.catalog.definitions))
                .expect("declared module is in graph")
                .clone();
            let declared = declare_analysis(
                lowered,
                imports,
                graph.catalog.clone(),
                &self.definitions,
                cancel,
            )?;
            let mut diagnostics = declared.record.names.diagnostics.clone();
            diagnostics.extend(parsed.diagnostics().cloned());
            let metadata =
                DefinitionMetadata::checked(self.definitions.snapshot(), declared, cancel)?;
            let definitions = metadata.definitions().clone();
            let mut declared = metadata.into_records();
            Arc::make_mut(&mut declared.record)
                .declarations
                .publish_definitions(definitions);
            let file = Arc::new(FileDeclarations {
                parsed,
                declared,
                diagnostics: Arc::new(diagnostics),
            });
            files.insert(id, file);
        }
        cancel.check()?;
        let snapshot = DeclarationSnapshot {
            native_modules: self.native_modules.clone(),
            revision: source.revision(),
            graph,
            files: Arc::new(files),
        };
        snapshot.validate_registered_exports(cancel)?;
        Ok(snapshot)
    }
}
