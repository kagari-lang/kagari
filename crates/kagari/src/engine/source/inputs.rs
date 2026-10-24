//! Detached compilation inputs; neither engine state nor executable evidence.

use crate::{
    CompileResult,
    engine::{
        packages::PackageBuildInput,
        source::{CheckedModule, PreparationOptions, analysis::SourceAnalysis, analysis_error},
    },
    error::EmbeddingError,
};
use kagari_bytecode::artifact::KAGARI_COMPILER_FINGERPRINT;
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_compiler::source::lower::instances::MirLoweringOptions;
use kagari_contract::source::MAX_SOURCE_BYTES;
use kagari_hir::analysis::{AnalysisSnapshot, inputs::AnalysisInputs};
use kagari_mir::passes::PassOptions;
use kagari_source::{
    identity::FileId,
    source_database::{CapturedSource, SourceDatabase, SourceSnapshot},
};
use std::{borrow::Cow, collections::BTreeSet, iter, sync::Arc};

/// Exact authored documents and declaration environment used by one source analysis.
/// Package acquisition is provenance, not permission to read sources again. After
/// edits, `sources` is authoritative; provenance still describes the original capture.
#[derive(Debug, Clone)]
pub(crate) struct CheckedInputs {
    sources: SourceSnapshot,
    environment: Arc<AnalysisInputs>,
    root: ModuleIdentity,
    provenance: Option<PackageBuildInput>,
    edited: bool,
    compiler_contract: &'static str,
}

/// Physical edits already resolved by the SDK batch owner. No function-target
/// resolution or partial-definition splicing occurs at this boundary.
#[derive(Debug, Clone)]
pub(crate) enum SourceEdit {
    /// Resolved physical document from this capsule, not public update authority.
    Document {
        file: FileId,
        text: String,
    },
    Replace {
        name: String,
        text: String,
    },
    Add {
        name: String,
        module: ModuleIdentity,
        text: String,
    },
}

struct DocumentInput<'a> {
    name: Cow<'a, str>,
    module: &'a ModuleIdentity,
    text: &'a str,
}

impl CheckedInputs {
    pub(super) fn capture(
        snapshot: &AnalysisSnapshot,
        root: FileId,
        provenance: Option<PackageBuildInput>,
    ) -> CompileResult<Arc<Self>> {
        let root = snapshot
            .file(root)
            .ok_or_else(|| source_error("file is absent from this source snapshot"))?;
        Ok(Arc::new(Self {
            sources: snapshot.physical_sources().clone(),
            environment: snapshot.inputs().clone(),
            root: root.source().module_identity().clone(),
            provenance,
            edited: false,
            compiler_contract: KAGARI_COMPILER_FINGERPRINT,
        }))
    }

    pub(crate) fn sources(&self) -> &SourceSnapshot {
        &self.sources
    }

    pub(crate) fn environment(&self) -> &Arc<AnalysisInputs> {
        &self.environment
    }

    pub(crate) fn root(&self) -> &ModuleIdentity {
        &self.root
    }

    pub(crate) fn provenance(&self) -> Option<&PackageBuildInput> {
        self.provenance.as_ref()
    }

    pub(crate) fn package_fingerprint(&self) -> Option<&str> {
        (!self.edited)
            .then(|| self.provenance.as_ref().map(PackageBuildInput::fingerprint))
            .flatten()
    }

    pub(crate) fn compiler_contract(&self) -> &str {
        self.compiler_contract
    }

    pub(crate) fn document_name(
        &self,
        name: &str,
        cancel: &CancellationToken,
    ) -> CompileResult<String> {
        self.database(cancel)?
            .source_name(name)
            .map_err(source_error)
    }

    fn database(&self, cancel: &CancellationToken) -> CompileResult<SourceDatabase> {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        SourceDatabase::new(self.environment().source_root()).map_err(source_error)
    }

    fn documents(&self, cancel: &CancellationToken) -> CompileResult<Vec<DocumentInput<'_>>> {
        self.sources
            .files()
            .map(|file| {
                cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
                // Synthetic views are derived by HIR, never replayed as physical files.
                if file.inline_range().is_some() {
                    return Err(source_error(
                        "compilation inputs contain a non-physical source view",
                    ));
                }
                Ok(DocumentInput {
                    name: Cow::Borrowed(file.name()),
                    module: file.module_identity(),
                    text: file.text(),
                })
            })
            .collect()
    }

    /// Forms one complete candidate without changing this capture or acquiring packages.
    /// Replacements preserve physical module mappings; additions must name an already
    /// captured package. Duplicate normalized document targets reject atomically.
    pub(crate) fn with_edits(
        &self,
        edits: &[SourceEdit],
        cancel: &CancellationToken,
    ) -> CompileResult<Arc<Self>> {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        let mut database = self.database(cancel)?;
        let mut documents = self.documents(cancel)?;
        let mut targets = BTreeSet::new();
        let mut modified = BTreeSet::new();
        for edit in edits {
            cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
            let name = match edit {
                SourceEdit::Document { file, text } => {
                    let index = self
                        .sources
                        .files()
                        .position(|source| source.id() == *file)
                        .ok_or_else(|| source_error("foreign captured document"))?;
                    if !modified.insert(index) {
                        return Err(source_error("duplicate physical source edit"));
                    }
                    documents[index].text = text;
                    continue;
                }
                SourceEdit::Replace { name, .. } | SourceEdit::Add { name, .. } => name,
            };
            let key = database.source_name(name).map_err(source_error)?;
            if !targets.insert(key.clone()) {
                return Err(source_error(format!(
                    "duplicate physical source edit: {name}"
                )));
            }
            let matches = documents
                .iter()
                .enumerate()
                .filter_map(|(index, document)| {
                    if cancel.check().is_err() {
                        return Some(Err(EmbeddingError::Cancelled));
                    }
                    match database.source_name(&document.name) {
                        Ok(name) if name == key => Some(Ok(index)),
                        Ok(_) => None,
                        Err(message) => Some(Err(source_error(message))),
                    }
                })
                .collect::<CompileResult<Vec<_>>>()?;
            if matches.len() > 1 {
                return Err(source_error(format!("ambiguous captured document: {name}")));
            }
            if let Some(index) = matches.first()
                && !modified.insert(*index)
            {
                return Err(source_error("duplicate physical source edit"));
            }
            let existing = matches.first().map(|index| &mut documents[*index]);
            match (edit, existing) {
                (SourceEdit::Document { .. }, _) => {
                    unreachable!("resolved documents handled above")
                }
                (SourceEdit::Replace { text, .. }, Some(document)) => document.text = text,
                (SourceEdit::Replace { .. }, None) => {
                    return Err(source_error(format!("unknown captured document: {name}")));
                }
                (SourceEdit::Add { .. }, Some(_)) => {
                    return Err(source_error(format!(
                        "captured document already exists: {name}"
                    )));
                }
                (SourceEdit::Add { module, text, .. }, None) => {
                    let captured = self.sources.packages().scope(&module.package).is_some()
                        || self
                            .sources
                            .files()
                            .any(|file| file.module_identity().package == module.package);
                    if !captured {
                        return Err(source_error(format!(
                            "cannot acquire package in source edit: {}",
                            module.package.0
                        )));
                    }
                    documents.push(DocumentInput {
                        name: Cow::Owned(key),
                        module,
                        text,
                    });
                }
            }
        }
        let documents = capture_documents(documents, cancel)?;
        let sources = database
            .replace_captured(documents, self.sources.packages().clone(), cancel)
            .map_err(|message| capture_error(message, cancel))?;
        Ok(Arc::new(Self {
            sources,
            environment: self.environment.clone(),
            root: self.root.clone(),
            provenance: self.provenance.clone(),
            edited: self.edited || !edits.is_empty(),
            compiler_contract: self.compiler_contract,
        }))
    }

    /// Reconstructs fresh source and analysis databases solely from immutable inputs.
    pub(crate) fn recheck(
        self: &Arc<Self>,
        cancel: &CancellationToken,
    ) -> CompileResult<CheckedModule> {
        self.analyze(cancel)?.check(cancel)
    }

    pub(crate) fn analyze(
        self: &Arc<Self>,
        cancel: &CancellationToken,
    ) -> CompileResult<SourceAnalysis> {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        let mut sources = self.database(cancel)?;
        let documents = capture_documents(self.documents(cancel)?, cancel)?;
        let source = sources
            .replace_captured(documents, self.sources.packages().clone(), cancel)
            .map_err(|message| capture_error(message, cancel))?;
        let mut database = self.environment.database().map_err(source_error)?;
        let snapshot = database.snapshot(source, cancel).map_err(analysis_error)?;
        let root = snapshot
            .declaration_snapshot()
            .files()
            .find(|file| file.source().module_identity() == &self.root)
            .ok_or_else(|| source_error(format!("captured root is absent: {}", self.root)))?
            .source()
            .id();
        let inputs = Arc::new(Self {
            sources: snapshot.physical_sources().clone(),
            ..self.as_ref().clone()
        });
        Ok(SourceAnalysis {
            snapshot,
            root,
            inputs,
        })
    }
}

/// Validate final text sizes before any replay clones, including unchanged documents.
/// Source-package verification separately accounts for its metadata overhead.
fn capture_documents(
    documents: Vec<DocumentInput<'_>>,
    cancel: &CancellationToken,
) -> CompileResult<Vec<CapturedSource>> {
    let mut budget = SourceTextBudget::default();
    for document in &documents {
        cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
        budget.add_document(document.text.len())?;
    }
    documents
        .into_iter()
        .enumerate()
        .map(|(index, document)| {
            cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
            Ok(CapturedSource {
                // Display origins can coincide across packages; keys must not collapse them.
                key: format!("capsule://document/{index}"),
                display_name: copy_source_text(&document.name, cancel)?,
                module: document.module.clone(),
                text: copy_source_text(document.text, cancel)?,
            })
        })
        .collect()
}

/// A preallocation check using the existing retained-source limit, not another quota.
#[derive(Default)]
pub(crate) struct SourceTextBudget {
    bytes: usize,
}

impl SourceTextBudget {
    pub(crate) fn add_document(&mut self, bytes: usize) -> CompileResult<()> {
        let total = self
            .bytes
            .checked_add(bytes)
            .filter(|total| bytes <= MAX_SOURCE_BYTES && *total <= MAX_SOURCE_BYTES)
            .ok_or_else(source_size_error)?;
        self.bytes = total;
        Ok(())
    }
}

/// Compute the complete result, not an order-dependent intermediate edit size.
pub(crate) fn replaced_source_len(
    original: usize,
    removed: usize,
    inserted: usize,
) -> CompileResult<usize> {
    original
        .checked_sub(removed)
        .and_then(|bytes| bytes.checked_add(inserted))
        .filter(|bytes| *bytes <= MAX_SOURCE_BYTES)
        .ok_or_else(source_size_error)
}

fn source_size_error() -> EmbeddingError {
    source_error("resulting source exceeds 64 MiB of retained source UTF-8")
}

pub(crate) fn source_text_buffer(
    bytes: usize,
    cancel: &CancellationToken,
) -> CompileResult<String> {
    cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
    let bytes = replaced_source_len(bytes, 0, 0)?;
    let mut text = String::new();
    text.try_reserve_exact(bytes)
        .map_err(|error| source_error(format!("cannot reserve source text: {error}")))?;
    Ok(text)
}

pub(crate) fn copy_source_text(text: &str, cancel: &CancellationToken) -> CompileResult<String> {
    let mut copied = source_text_buffer(text.len(), cancel)?;
    append_source_text(&mut copied, text, cancel)?;
    Ok(copied)
}

pub(crate) fn append_source_text(
    output: &mut String,
    text: &str,
    cancel: &CancellationToken,
) -> CompileResult<()> {
    cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
    for chunk in source_text_chunks(text, cancel) {
        output.push_str(chunk?);
    }
    cancel.check().map_err(|_| EmbeddingError::Cancelled)
}

fn source_text_chunks<'a>(
    mut text: &'a str,
    cancel: &'a CancellationToken,
) -> impl Iterator<Item = CompileResult<&'a str>> {
    // Checkpoint interval only: the source limit above owns the resource policy.
    const COPY_CHUNK_BYTES: usize = 64 * 1024;
    iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        if cancel.check().is_err() {
            text = "";
            return Some(Err(EmbeddingError::Cancelled));
        }
        let mut end = text.len().min(COPY_CHUNK_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let (chunk, rest) = text.split_at(end);
        text = rest;
        Some(Ok(chunk))
    })
}

/// Semantic lowering settings intentionally have no request cancellation token.
#[derive(Debug, Clone)]
struct LoweringSettings {
    optimization: Option<PassOptions>,
    max_generic_instances: usize,
    max_type_nodes: usize,
    max_type_depth: usize,
    max_instructions: usize,
}

/// Send + Sync source preparation capsule, independent of PreparedProgram's caches.
#[derive(Debug, Clone)]
pub(crate) struct CompilationInputs {
    checked: Arc<CheckedInputs>,
    lowering: LoweringSettings,
    #[cfg(feature = "native")]
    retain_native_input: bool,
}

impl CompilationInputs {
    pub(crate) fn capture(checked: Arc<CheckedInputs>, options: &MirLoweringOptions) -> Self {
        let MirLoweringOptions {
            optimization,
            max_generic_instances,
            max_type_nodes,
            max_type_depth,
            max_instructions,
            cancel: _,
        } = options;
        Self {
            checked,
            #[cfg(feature = "native")]
            retain_native_input: false,
            lowering: LoweringSettings {
                optimization: optimization.clone(),
                max_generic_instances: *max_generic_instances,
                max_type_nodes: *max_type_nodes,
                max_type_depth: *max_type_depth,
                max_instructions: *max_instructions,
            },
        }
    }

    pub(crate) fn with_preparation(&self, options: &PreparationOptions) -> Self {
        #[cfg(not(feature = "native"))]
        let _ = options;
        Self {
            #[cfg(feature = "native")]
            retain_native_input: options.retain_native_input,
            ..self.clone()
        }
    }

    pub(crate) fn preparation_options(&self, cancel: CancellationToken) -> PreparationOptions {
        PreparationOptions {
            lowering: self.lowering(cancel),
            #[cfg(feature = "native")]
            retain_native_input: self.retain_native_input,
        }
    }

    pub(crate) fn checked(&self) -> &Arc<CheckedInputs> {
        &self.checked
    }

    /// Supplies cancellation for this request, never the original preparation's token.
    pub(crate) fn lowering(&self, cancel: CancellationToken) -> MirLoweringOptions {
        MirLoweringOptions {
            optimization: self.lowering.optimization.clone(),
            max_generic_instances: self.lowering.max_generic_instances,
            max_type_nodes: self.lowering.max_type_nodes,
            max_type_depth: self.lowering.max_type_depth,
            max_instructions: self.lowering.max_instructions,
            cancel,
        }
    }

    pub(crate) fn with_edits(
        &self,
        edits: &[SourceEdit],
        cancel: &CancellationToken,
    ) -> CompileResult<Self> {
        Ok(Self {
            checked: self.checked.with_edits(edits, cancel)?,
            lowering: self.lowering.clone(),
            #[cfg(feature = "native")]
            retain_native_input: self.retain_native_input,
        })
    }
}

fn source_error(message: impl Into<String>) -> EmbeddingError {
    EmbeddingError::Source {
        message: message.into(),
    }
}

fn capture_error(message: String, cancel: &CancellationToken) -> EmbeddingError {
    if cancel.check().is_err() {
        EmbeddingError::Cancelled
    } else {
        source_error(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{
        KagariEngine,
        source::{PreparationOptions, compilation::Compilation},
    };
    use kagari_common::identity::PackageId;
    use kagari_hir::typeck::const_budget::ConstLimits;
    use kagari_package::{modules::VirtualSources, project::ProjectInputs};
    use kagari_runtime::native::{
        builder::ModuleBuilder, context::CallContext, declarations::FunctionDecl, types::Type,
    };
    use kagari_source::{
        package::{PackageEnvironment, PackageScope},
        source::SourceFile,
    };
    use kagari_syntax::parser::ParseLimits;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn resulting_source_size_preflight_counts_retained_replacements_and_additions() {
        const MIB: usize = 1024 * 1024;
        // Replacement bytes alone fit; the retained comments make the result too big.
        assert!(replaced_source_len(40 * MIB + 10, 10, 32 * MIB).is_err());
        assert_eq!(
            replaced_source_len(40 * MIB + 10, 10, 24 * MIB).unwrap(),
            MAX_SOURCE_BYTES
        );
        assert_eq!(
            replaced_source_len(MAX_SOURCE_BYTES, MAX_SOURCE_BYTES, "π🦀".len()).unwrap(),
            6
        );
        assert!(replaced_source_len(1, 2, 0).is_err());
        assert!(replaced_source_len(usize::MAX, 0, 1).is_err());
        assert!(replaced_source_len(MAX_SOURCE_BYTES, 0, 1).is_err());
        assert!(source_text_buffer(MAX_SOURCE_BYTES + 1, &CancellationToken::default()).is_err());

        let mut budget = SourceTextBudget::default();
        budget.add_document(40 * MIB).unwrap(); // Retained, unedited physical document.
        budget.add_document(24 * MIB).unwrap(); // Whole-file replacement or added file.
        assert!(budget.add_document(1).is_err());
        assert!(
            SourceTextBudget::default()
                .add_document(MAX_SOURCE_BYTES + 1)
                .is_err()
        );
        assert!(budget.add_document(usize::MAX).is_err());
        assert_eq!(
            budget.bytes, MAX_SOURCE_BYTES,
            "rejection must not advance accounting"
        );
    }

    #[test]
    fn source_text_copy_observes_cancellation_between_utf8_chunks() {
        let text = format!("{}🦀{}", "a".repeat(64 * 1024 - 1), "π".repeat(64 * 1024));
        let cancel = CancellationToken::default();
        let mut chunks = source_text_chunks(&text, &cancel);
        assert_eq!(chunks.next().unwrap().unwrap().len(), 64 * 1024 - 1);
        cancel.cancel();
        assert!(matches!(
            chunks.next(),
            Some(Err(EmbeddingError::Cancelled))
        ));
        assert!(chunks.next().is_none());
        assert!(matches!(
            copy_source_text(&text, &cancel),
            Err(EmbeddingError::Cancelled)
        ));
        assert_eq!(
            copy_source_text(&text, &CancellationToken::default()).unwrap(),
            text
        );
        assert!(matches!(
            copy_source_text("", &cancel),
            Err(EmbeddingError::Cancelled)
        ));
    }

    fn execute(engine: &KagariEngine, checked: &CheckedModule) -> i32 {
        let prepared = engine
            .prepare(checked, PreparationOptions::default())
            .unwrap();
        let mut runtime = engine.runtime(Default::default());
        let program = runtime.load_program(&prepared, Default::default()).unwrap();
        runtime
            .execute_typed::<(), i32>(&program, "answer", (), &Default::default())
            .unwrap()
    }

    #[test]
    fn capsule_is_send_sync_and_detaches_settings_declarations_and_callbacks() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<CheckedInputs>();
        send_sync::<CompilationInputs>();
        let calls = Arc::new(AtomicUsize::new(0));
        let weak = Arc::downgrade(&calls);
        let mut builder = KagariEngine::builder().unwrap();
        builder.source_root("/capsule").unwrap();
        let mut native = ModuleBuilder::new("host", builder.declarations()).unwrap();
        let function = native
            .define_function(FunctionDecl::new("number").returns(Type::i32()))
            .unwrap();
        native
            .bind(function, move |_: &mut CallContext<'_>| {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(42i32)
            })
            .unwrap();
        builder.install(native.finish().unwrap()).unwrap();
        let engine = builder.build().unwrap();
        let const_limits = ConstLimits {
            max_steps: 1234,
            max_depth: 31,
        };
        let parse_limits = ParseLimits {
            max_nesting: 50,
            ..Default::default()
        };
        engine.set_const_limits(const_limits);
        engine.set_parse_limits(parse_limits);
        engine.set_max_semantic_diagnostics(23);
        let cancel = CancellationToken::default();
        let analysis = engine
            .analyze_source(
                SourceFile::new("main.kgr", "pub fn answer() -> i32 { host::number() }"),
                &cancel,
            )
            .unwrap();
        let environment = analysis.inputs().environment().clone();
        // Change the host session between analysis and its strict checked-program gate.
        engine.set_const_limits(Default::default());
        engine.set_parse_limits(Default::default());
        engine.set_max_semantic_diagnostics(1);
        engine.analysis.borrow_mut().set_native_modules(vec![]);
        let checked = analysis.check(&cancel).unwrap();
        assert!(Arc::ptr_eq(checked.inputs().environment(), &environment));
        let options = MirLoweringOptions {
            max_generic_instances: 1025,
            max_type_nodes: 8193,
            max_type_depth: 65,
            max_instructions: 1_000_001,
            cancel: cancel.clone(),
            optimization: Some(PassOptions {
                max_work: 1_000_001,
            }),
        };
        let compilation = Compilation::lower(&checked, &options).unwrap();
        let capsule = compilation.inputs().clone();
        cancel.cancel();
        drop(engine);
        assert!(
            weak.upgrade().is_none(),
            "capsule must not retain callback captures"
        );
        assert_eq!(capsule.checked().environment().const_limits(), const_limits);
        assert_eq!(capsule.checked().environment().parse_limits(), parse_limits);
        assert_eq!(
            capsule.checked().environment().max_semantic_diagnostics(),
            23
        );
        assert_eq!(capsule.checked().environment().source_root(), "/capsule");
        assert_eq!(
            capsule.checked().compiler_contract(),
            KAGARI_COMPILER_FINGERPRINT
        );
        let request = CancellationToken::default();
        let lowering = capsule.lowering(request.clone());
        assert!(lowering.cancel.check().is_ok());
        assert_eq!(
            (
                lowering.max_generic_instances,
                lowering.max_type_nodes,
                lowering.max_type_depth,
                lowering.max_instructions
            ),
            (1025, 8193, 65, 1_000_001)
        );
        assert_eq!(lowering.optimization.as_ref().unwrap().max_work, 1_000_001);
        let rebuilt = capsule.checked().recheck(&request).unwrap();
        Compilation::lower(&rebuilt, &lowering).unwrap();
        assert!(matches!(
            capsule.checked().recheck(&cancel),
            Err(EmbeddingError::Cancelled)
        ));
    }

    #[test]
    fn direct_candidates_recheck_whole_inputs_without_changing_the_engine() {
        let engine = KagariEngine::default();
        let checked = engine
            .compile_source(SourceFile::new(
                "main.kgr",
                "pub fn answer() -> i32 { 40 + 2 }",
            ))
            .unwrap();
        let capsule = CompilationInputs::capture(checked.inputs().clone(), &Default::default());
        let cancel = CancellationToken::default();
        let rebuilt = capsule.checked().recheck(&cancel).unwrap();
        assert_eq!(execute(&engine, &checked), execute(&engine, &rebuilt));
        let name = checked.source_name.clone();
        let candidate = capsule
            .with_edits(
                &[SourceEdit::Replace {
                    name: name.clone(),
                    text: "pub fn answer() -> i32 { 43 }".into(),
                }],
                &cancel,
            )
            .unwrap();
        let changed = candidate.checked().recheck(&cancel).unwrap();
        assert_eq!(execute(&engine, &changed), 43);
        assert_eq!(execute(&engine, &checked), 42);
        assert!(
            engine
                .source_snapshot()
                .files()
                .next()
                .unwrap()
                .text()
                .contains("40 + 2")
        );
        assert_eq!(candidate.checked().root(), checked.module_identity());
        assert!(candidate.checked().provenance().is_none());
        for text in ["pub fn answer() -> i32 { true }", "pub fn answer( {"] {
            let invalid = capsule
                .with_edits(
                    &[SourceEdit::Replace {
                        name: name.clone(),
                        text: text.into(),
                    }],
                    &cancel,
                )
                .unwrap();
            let error = invalid.checked().recheck(&cancel).unwrap_err();
            let direct = engine
                .compile_source(SourceFile::new(&name, text))
                .unwrap_err();
            let (
                EmbeddingError::Diagnostics {
                    diagnostics: rebuilt,
                },
                EmbeddingError::Diagnostics {
                    diagnostics: direct,
                },
            ) = (error, direct)
            else {
                panic!("both paths must report source diagnostics");
            };
            assert_eq!(rebuilt.len(), direct.len());
            for (rebuilt, direct) in rebuilt.iter().zip(&direct) {
                assert_eq!(
                    (
                        &rebuilt.code,
                        rebuilt.severity,
                        &rebuilt.message,
                        rebuilt.span.map(|span| span.range),
                        &rebuilt.notes
                    ),
                    (
                        &direct.code,
                        direct.severity,
                        &direct.message,
                        direct.span.map(|span| span.range),
                        &direct.notes
                    )
                );
                assert_eq!(
                    rebuilt
                        .labels
                        .iter()
                        .map(|label| (label.span.range, &label.message))
                        .collect::<Vec<_>>(),
                    direct
                        .labels
                        .iter()
                        .map(|label| (label.span.range, &label.message))
                        .collect::<Vec<_>>()
                );
            }
        }
        let foreign = ModuleIdentity {
            package: PackageId("not-captured".into()),
            path: vec![],
        };
        for edits in [
            vec![SourceEdit::Replace {
                name: "absent.kgr".into(),
                text: String::new(),
            }],
            vec![
                SourceEdit::Replace {
                    name: name.clone(),
                    text: String::new(),
                },
                SourceEdit::Replace {
                    name: name.clone(),
                    text: String::new(),
                },
            ],
            vec![SourceEdit::Add {
                name: "new.kgr".into(),
                module: foreign,
                text: String::new(),
            }],
        ] {
            assert!(matches!(
                capsule.with_edits(&edits, &cancel),
                Err(EmbeddingError::Source { .. })
            ));
        }
    }

    #[test]
    fn package_candidates_keep_aliases_mappings_and_acquisition_provenance() {
        let cancel = CancellationToken::default();
        let sources = VirtualSources::new([
            ("Kagari.toml".into(), Arc::from("[package]\nname = 'app'\nnamespace = 'tests/capsule'\nversion = '1.0.0'\n[dependencies]\nrules = { path = 'rules' }")),
            ("src/lib.kgr".into(), Arc::from("pub fn answer() -> i32 { rules::value() }")),
            ("rules/Kagari.toml".into(), Arc::from("[package]\nname = 'rules'\nnamespace = 'tests/capsule-rules'\nversion = '1.0.0'")),
            ("rules/src/lib.kgr".into(), Arc::from("pub fn value() -> i32 { 42 }")),
        ], &cancel).unwrap();
        let project = ProjectInputs::load(&sources, "", &Default::default(), &cancel).unwrap();
        let package = project.programs().values().next().unwrap();
        let engine = KagariEngine::default();
        let checked = engine.compile_package(package, &cancel).unwrap();
        let capture = checked.inputs();
        assert_eq!(capture.sources().files().count(), 2);
        let rebuilt = capture.recheck(&cancel).unwrap();
        assert_eq!(execute(&engine, &checked), execute(&engine, &rebuilt));
        assert_eq!(checked.package_fingerprint(), rebuilt.package_fingerprint());
        let root = capture.sources().module(capture.root()).unwrap();
        let mut added = capture.root().clone();
        added.path.push("extra".into());
        let candidate = capture
            .with_edits(
                &[
                    SourceEdit::Replace {
                        name: root.name().into(),
                        text:
                            "mod extra; pub fn answer() -> i32 { rules::value() + extra::bonus() }"
                                .into(),
                    },
                    SourceEdit::Add {
                        name: "src/extra.kgr".into(),
                        module: added.clone(),
                        text: "pub fn bonus() -> i32 { 1 }".into(),
                    },
                ],
                &cancel,
            )
            .unwrap();
        assert_eq!(candidate.sources().packages(), capture.sources().packages());
        assert!(candidate.sources().module(&added).is_some());
        let changed = candidate.recheck(&cancel).unwrap();
        assert_eq!(execute(&engine, &changed), 43);
        assert_eq!(
            changed.package_inputs().unwrap().fingerprint(),
            package.fingerprint()
        );
        assert!(
            changed.package_fingerprint().is_none(),
            "old acquisition fingerprint must not identify edited bytes"
        );
        assert_eq!(execute(&engine, &checked), 42);
    }

    #[test]
    fn explicit_snapshot_replay_does_not_collapse_equal_display_origins() {
        let engine = KagariEngine::default();
        let cancel = CancellationToken::default();
        let root = ModuleIdentity {
            package: PackageId("app".into()),
            path: vec![],
        };
        let dep = ModuleIdentity {
            package: PackageId("rules".into()),
            path: vec![],
        };
        let packages = PackageEnvironment::new(
            [
                (
                    root.package.clone(),
                    PackageScope {
                        dependencies: [("rules".into(), dep.package.clone())].into(),
                        selection: "app-v1".into(),
                    },
                ),
                (
                    dep.package.clone(),
                    PackageScope {
                        dependencies: Default::default(),
                        selection: "rules-v1".into(),
                    },
                ),
            ]
            .into(),
        )
        .unwrap();
        let mut database = SourceDatabase::new("/capsule").unwrap();
        let snapshot = database
            .replace_captured(
                vec![
                    CapturedSource {
                        key: "host://app".into(),
                        display_name: "src/lib.kgr".into(),
                        module: root.clone(),
                        text: "pub fn answer() -> i32 { rules::value() }".into(),
                    },
                    CapturedSource {
                        key: "host://rules".into(),
                        display_name: "src/lib.kgr".into(),
                        module: dep,
                        text: "pub fn value() -> i32 { 42 }".into(),
                    },
                ],
                packages,
                &cancel,
            )
            .unwrap();
        let file = snapshot.module(&root).unwrap().id();
        let checked = engine.compile_snapshot(snapshot, file, &cancel).unwrap();
        let rebuilt = checked.inputs().recheck(&cancel).unwrap();
        assert_eq!(execute(&engine, &rebuilt), 42);
        assert_eq!(rebuilt.inputs().sources().files().count(), 2);
        assert!(rebuilt.package_inputs().is_none());
        assert!(matches!(
            checked.inputs().with_edits(
                &[SourceEdit::Replace {
                    name: "src/lib.kgr".into(),
                    text: String::new()
                }],
                &cancel
            ),
            Err(EmbeddingError::Source { .. })
        ));
    }
}
