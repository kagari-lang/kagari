//! Protocol-independent immutable source analysis. Queries never execute code.

pub mod core_source;
mod environment;
#[cfg(test)]
pub(crate) mod external;
mod input_fingerprint;
pub mod inputs;
pub(crate) mod records;
pub(crate) mod registered_fields;
pub(crate) mod source_preparation;

use crate::imports::environment::ImportEnvironment;
#[cfg(test)]
use crate::tests::cancellation::{Checkpoint, checkpoint};

#[cfg(test)]
use crate::imports::solver::SolverLimits;
use crate::{
    AnalysisPolicy, AnalysisResult, AnalyzedModule,
    analysis::{
        declarations::DeclarationSnapshot, error::AnalysisError, inputs::AnalysisInputs,
        signatures::SignatureSnapshot, source_preparation::ParsedModule,
    },
    analyze_parsed,
    collections::HashMap,
    declarations::{Declaration, DeclarationId},
    imports::ModuleGraph,
    native::api::preparation::NativeFiles,
    typeck::{ModuleSignatures, const_budget::ConstLimits},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext, mapping::DefinitionMappingError, metadata::DefinitionMetadata,
        table::DefinitionTable,
    },
};
use kagari_source::{
    diagnostic::Diagnostic,
    identity::{FileId, Revision},
    source::SourceFile,
    source_database::{SourceDatabase, SourceDatabaseError, SourceSnapshot},
};
use kagari_syntax::{ast::module::ModuleSyntax, parser::ParseLimits};
use kagari_types::declaration::module::ModuleDecl;
use std::{cell::OnceCell, collections::BTreeMap, sync::Arc};

const DEFAULT_MAX_SEMANTIC_DIAGNOSTICS: usize = 1_000;

pub mod error;

pub mod body;
pub mod eligibility;

pub mod declarations;
pub(crate) mod ownership;
pub mod signatures;

/// One file's parsed source and completed semantic analysis, retained by a snapshot.
///
/// `result` owns recoverable semantic facts, diagnostics and their definition table;
/// completed analysis does not imply error-free source or compilation eligibility.
/// Local HIR IDs index `result().facts().lowered`; scoped definition IDs resolve through
/// [`Self::definitions`]. Editor projections are owned by kagari-analyzer.
#[derive(Debug)]
pub struct FileAnalysis {
    /// Borrows the immutable source revision analyzed by this result.
    source: Arc<SourceFile>,
    shared_valid: bool,

    parsed: ParsedModule,
    /// Semantic facts and diagnostics paired with the table owning their definition IDs.
    result: DefinitionMetadata<AnalysisResult<AnalyzedModule>>,
}

impl FileAnalysis {
    /// Borrows the shared signature-stage result used to check this file.
    pub fn signatures(&self) -> &Arc<AnalysisResult<ModuleSignatures>> {
        &self.result.records().facts().signatures
    }

    /// Returns the retained source AST, or `None` for a structured declaration input.
    pub fn syntax(&self) -> Option<ModuleSyntax> {
        self.parsed.syntax()
    }

    /// Borrows the immutable source revision analyzed by this result.
    pub fn source(&self) -> &SourceFile {
        &self.source
    }

    /// Semantic prefix before this logical view's appended parse diagnostics.
    pub(crate) fn semantic_diagnostics(&self) -> &[Diagnostic] {
        let diagnostics = self.result().diagnostics();
        &diagnostics[..diagnostics.len() - self.parsed.diagnostics().len()]
    }

    /// Borrows recoverable semantic facts and diagnostics, including body analysis.
    pub fn result(&self) -> &AnalysisResult<AnalyzedModule> {
        self.result.records()
    }
}

/// Mutable owner of analysis inputs and scoped identities.
///
/// Choose the required depth explicitly:
///
/// ```text
/// SourceSnapshot + authored core documents + scoped registrations + import environment
///   declarations() -> DeclarationSnapshot: parse, lower, imports, declaration names
///   signatures()   -> SignatureSnapshot: declarations + checked signatures
///   body(owner)    -> FunctionAnalysis: signatures + selected body/constant prerequisites
///   snapshot()     -> AnalysisSnapshot: signatures + all file bodies
/// ```
///
/// The last two entrypoints check bodies before returning; snapshot position
/// queries are reads of prepared facts. Analysis errors describe cancellation, import
/// solver failure or invalid inputs/identity metadata; ordinary source errors are retained as diagnostics.
///
/// Each request computes source facts from its current inputs. Clones share the
/// definition context and can share validated Native inputs, not previous analysis
/// results. Returned snapshots retain the exact source, records and identity owner.
#[derive(Debug, Clone)]
pub struct AnalysisDatabase {
    source_root: String,
    #[cfg(test)]
    pub(crate) import_solver_limits: Option<SolverLimits>,
    /// Database identity context used to scope portable declaration paths.
    definitions: DefinitionContext,
    const_limits: ConstLimits,
    parse_limits: ParseLimits,
    max_semantic_diagnostics: usize,
    /// Stable synthetic file IDs keyed by parent file and inline-module name.
    inline_ids: HashMap<(FileId, String), FileId>,
    import_environment: ImportEnvironment,
    native_modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    core: Arc<[Arc<SourceFile>]>,
    /// Validated, adopted registration metadata.
    /// Invalidated by registration or importer spelling changes, not source revisions.
    native_files: OnceCell<NativeFiles>,
    /// Portable provider/settings identity, invalidated by every environment setter.
    environment_fingerprint: OnceCell<String>,
}

impl Default for AnalysisDatabase {
    fn default() -> Self {
        Self::new(SourceDatabase::default().root()).expect("captured source root")
    }
}

impl AnalysisDatabase {
    /// Create analysis with packaged core and an explicit absolute root for native origins.
    /// Core resources are shared; parsing and semantic checking start only on query.
    ///
    /// No filesystem access or working-directory lookup is performed. Relative
    /// declaration names use this captured root; virtual URI identities are preserved.
    /// Invalid or relative roots return an error. User snapshots retain their own names.
    pub fn new(source_root: &str) -> Result<Self, SourceDatabaseError> {
        let sources = SourceDatabase::new(source_root)?;
        let definitions = DefinitionContext::new().expect("analysis definition context exhausted");
        Ok(Self {
            source_root: sources.root().to_owned(),
            #[cfg(test)]
            import_solver_limits: None,
            definitions,
            parse_limits: Default::default(),
            const_limits: Default::default(),
            max_semantic_diagnostics: DEFAULT_MAX_SEMANTIC_DIAGNOSTICS,
            inline_ids: HashMap::default(),
            import_environment: Default::default(),
            native_modules: vec![],
            core: core_source::shared_modules(),
            native_files: OnceCell::new(),
            environment_fingerprint: OnceCell::new(),
        })
    }
}

impl AnalysisDatabase {
    /// Native declarations are explicit snapshot inputs. Existing snapshots keep their owners.
    /// The first declaration query imports the complete batch into this database's context;
    /// providers may originate in independent identity scopes.
    pub fn set_native_modules(&mut self, modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>) {
        self.environment_fingerprint.take();
        self.native_modules = modules;
        self.native_files.take();
    }

    /// Sets the per-analysis diagnostic budget.
    pub fn set_max_semantic_diagnostics(&mut self, limit: usize) {
        if self.max_semantic_diagnostics != limit {
            self.environment_fingerprint.take();
            self.max_semantic_diagnostics = limit;
        }
    }

    /// Sets constant-evaluation limits.
    pub fn set_const_limits(&mut self, limits: ConstLimits) {
        if self.const_limits != limits {
            self.environment_fingerprint.take();
            self.const_limits = limits;
        }
    }

    /// Limits are query inputs. Existing immutable snapshots retain their facts;
    /// subsequent requests use the new limits.
    pub fn set_parse_limits(&mut self, limits: ParseLimits) {
        if self.parse_limits == limits {
            return;
        }
        self.parse_limits = limits;
        self.environment_fingerprint.take();
    }

    /// Prepares signatures and checks bodies for every supplied/discovered file.
    ///
    /// Returns an immutable result even when source diagnostics exist. Use
    /// [`AnalysisSnapshot::check_program`] to require an error-free dependency closure.
    ///
    /// # Errors
    ///
    /// Returns [`AnalysisError`] on cancellation, invalid installed declarations or
    /// identity mapping or import solver failure. Failed preparation is not published as a new
    /// complete analysis snapshot.
    pub fn snapshot(
        &mut self,
        source: SourceSnapshot,
        cancel: &CancellationToken,
    ) -> Result<AnalysisSnapshot, AnalysisError> {
        cancel.check()?;
        let signature_snapshot = self.signatures(source.clone(), cancel)?;
        let shared_valid = signature_snapshot.shared_inputs_valid();
        let graph = signature_snapshot.declarations.graph.clone();
        let signatures = signature_snapshot
            .files
            .iter()
            .map(|(id, file)| {
                (
                    *id,
                    (
                        file.prepared.lowered.source.clone(),
                        file.declaration.parsed.clone(),
                        file.prepared.clone(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut environments = signature_snapshot.body_environments(self.const_limits, cancel)?;
        let mut files = HashMap::default();
        for (id, (file, parsed, scoped_prepared)) in signatures {
            cancel.check()?;
            let prepared = scoped_prepared;
            let environment = environments.remove(&id).expect("prepared body environment");
            let result = analyze_parsed(
                prepared,
                &parsed,
                AnalysisPolicy {
                    const_limits: self.const_limits,
                    max_semantic_diagnostics: self.max_semantic_diagnostics,
                },
                environment,
                cancel,
            )?;
            let metadata = ownership::scope(&result, &self.definitions, cancel)?;
            let definitions = metadata.definitions().clone();
            let mut records = metadata.into_records();
            records
                .facts
                .declarations
                .publish_definitions(definitions.clone());

            let result = DefinitionMetadata::checked(definitions, records, cancel)?;
            let analysis = Arc::new(FileAnalysis {
                shared_valid,
                source: file,
                parsed,
                result,
            });
            files.insert(id, analysis);
        }
        #[cfg(test)]
        checkpoint(Checkpoint::Publish);
        cancel.check()?;
        Ok(AnalysisSnapshot {
            inputs: self.inputs(),
            source: source.clone(),
            definitions: self.definitions.snapshot(),
            signatures: signature_snapshot,
            revision: source.revision(),
            graph,
            files: Arc::new(files),
        })
    }
}

/// Immutable, fully prepared analysis results from one source snapshot.
///
/// ```text
/// AnalysisSnapshot
///   signatures --shared results--> SignatureSnapshot --> DeclarationSnapshot
///   graph      --Arc-------------> ModuleGraph
///   files      --Arc<HashMap>----> FileId -> Arc<FileAnalysis>
///                                            parse + HIR + semantic side tables
///   definitions ----------------> scoped DefinitionId lookup table
/// ```
///
/// Created eagerly by [`AnalysisDatabase::snapshot`] without editor indexes.
/// Analyzer views select logical modules and derive cursor queries from these facts.
/// Analysis does not execute scripts. [`Self::check_program`] validates a root's reachable
/// closure for compiler handoff. Keeping a snapshot keeps its source and facts alive
/// across later database edits.
#[derive(Debug, Clone)]
pub struct AnalysisSnapshot {
    inputs: Arc<AnalysisInputs>,
    source: SourceSnapshot,
    definitions: DefinitionTable,
    signatures: SignatureSnapshot,
    /// Returns the revision of the input source snapshot.
    revision: Revision,
    graph: Arc<ModuleGraph>,
    files: Arc<HashMap<FileId, Arc<FileAnalysis>>>,
}

impl AnalysisSnapshot {
    /// Declaration environment and settings captured when these facts were checked.
    pub fn inputs(&self) -> &Arc<AnalysisInputs> {
        &self.inputs
    }

    /// Exact supplied documents, excluding derived inline and native declaration views.
    pub fn physical_sources(&self) -> &SourceSnapshot {
        &self.source
    }

    /// An immutable prefix of the database's explicit identity context.
    /// Local handles additionally retain their analysis/arena ownership.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    /// The declaration query consumed by this complete analysis.
    pub fn declaration_snapshot(&self) -> &DeclarationSnapshot {
        self.signatures.declaration_snapshot()
    }

    /// Borrows the signature snapshot used to prepare all file bodies.
    pub fn signature_snapshot(&self) -> &SignatureSnapshot {
        &self.signatures
    }

    /// Borrows the import graph built for this source snapshot.
    pub fn module_graph(&self) -> &ModuleGraph {
        &self.graph
    }

    /// Returns the revision of the input source snapshot.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Finds a physical or synthetic file analysis by its exact file ID.
    pub fn file(&self, id: FileId) -> Option<&Arc<FileAnalysis>> {
        self.files.get(&id)
    }

    /// Enumerates the exact logical module results retained by this snapshot.
    pub fn files(&self) -> impl Iterator<Item = &Arc<FileAnalysis>> {
        self.files.values()
    }

    /// Finds a declaration in this snapshot; foreign or absent IDs return `None`.
    pub fn declaration(&self, id: &DeclarationId) -> Option<&Declaration> {
        let id = ownership::locate(id, self.definitions())?;
        self.files
            .values()
            .find_map(|file| file.result.records().facts().declarations.get(&id))
    }

    /// Read a source owned by this analysis, including its installed standard package.
    pub fn source(&self, file: FileId) -> Option<&SourceFile> {
        self.declaration_snapshot().source(file)
    }
}

impl FileAnalysis {
    /// Borrows the table that owns scoped definition IDs in this file result.
    pub fn definitions(&self) -> &DefinitionTable {
        self.result.definitions()
    }

    /// Copy editable facts while retaining their definition scope and no executable evidence.
    pub fn to_unverified(
        &self,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<AnalysisResult<AnalyzedModule>>, DefinitionMappingError> {
        DefinitionMetadata::checked(
            self.definitions().clone(),
            self.result.records().clone(),
            cancel,
        )
    }
}

#[cfg(test)]
mod tests;
