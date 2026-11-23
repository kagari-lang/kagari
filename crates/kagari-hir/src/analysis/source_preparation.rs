//! Physical parse retention and logical inline source preparation.

use crate::{
    analysis::{
        error::AnalysisError,
        external::LibraryLowering,
        inputs::LibrarySourceKey,
        records::{DeclarationCache, DeclarationCacheEntry},
    },
    collections::{HashMap, HashSet},
    lower::{LoweredModule, lower_module_controlled, supplied_module_syntax},
};
use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    identity::{FileId, Revision},
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_syntax::{
    ast::{
        module::{ModuleLocation, ModuleSyntax},
        views::ItemView,
    },
    parser::{Parse, ParseLimits, ParseOptions, parse},
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

/// Database-owned physical parses, independent of declaration and lowering owners.
///
/// Preparation borrows this exclusively through [`SourcePreparer`]. Cloning forks
/// the index while sharing immutable source/parse allocations. Only a
/// complete source-syntax batch may replace it; older snapshots cannot evict
/// newer entries. Parser-limit setters must call [`Self::clear`]. Native declaration
/// parses remain owned by the existing native import path, not this cache.
#[derive(Debug, Clone, Default)]
pub(super) struct SourceParseCache {
    revision: Revision,
    files: BTreeMap<FileId, PhysicalParse>,
}

#[derive(Debug, Clone)]
struct PhysicalParse {
    source: Arc<SourceFile>,
    limits: ParseLimits,
    parsed: Arc<Parse>,
}

impl PhysicalParse {
    fn matches(&self, source: &Arc<SourceFile>, limits: ParseLimits) -> bool {
        // Equal file IDs and revisions need not carry identical copied-source metadata.
        Arc::ptr_eq(&self.source, source) && self.limits == limits
    }
}

impl SourceParseCache {
    /// Invalidates parses and the publication watermark after parser inputs change.
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    fn get_or_parse(
        &self,
        source: &Arc<SourceFile>,
        limits: ParseLimits,
        published: Option<&ParsedModule>,
        cancel: &CancellationToken,
    ) -> Result<PhysicalParse, Cancelled> {
        // A newer syntax batch can publish before its semantic analysis is cancelled.
        // Prefer the matching published declaration's parse, preserving its locator identity.
        if let Some(previous) = published
            .and_then(ParsedModule::source_parse)
            .filter(|previous| previous.matches(source, limits))
            .or_else(|| {
                self.files
                    .get(&source.id())
                    .filter(|entry| entry.matches(source, limits))
            })
        {
            return Ok(previous.clone());
        }
        Ok(PhysicalParse {
            source: source.clone(),
            limits,
            parsed: Arc::new(parse(source, ParseOptions { limits }, cancel)?),
        })
    }

    fn publish(
        &mut self,
        revision: Revision,
        files: BTreeMap<FileId, PhysicalParse>,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        cancel.check()?;
        if revision >= self.revision {
            self.revision = revision;
            self.files = files;
        }
        Ok(())
    }
}

/// Exclusive source-preparation state, independent of semantic cache owners.
pub(super) struct SourcePreparer<'a> {
    pub(super) cache: &'a mut SourceParseCache,
    pub(super) inline_ids: &'a mut HashMap<(FileId, String), FileId>,
    pub(super) limits: ParseLimits,
}

/// A logical view paired with the exact immutable parse that owns its locator.
#[derive(Debug, Clone)]
pub(crate) struct ParsedModule {
    input: ParsedInput,
    diagnostics: Arc<[usize]>,
}

#[derive(Debug, Clone)]
enum ParsedInput {
    Source {
        physical: PhysicalParse,
        key: LoweringInputKey,
    },
    Native(Arc<Parse>),
    Structured,
}

/// Exact source lowering input. The locator already qualifies its module by the parse.
/// Source metadata is derived from that physical input and the selected module.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LoweringInputKey {
    file: FileId,
    location: ModuleLocation,
    attachments: Option<LibrarySourceKey>,
}

impl ParsedModule {
    pub(crate) fn root(parsed: Option<Parse>) -> Self {
        match parsed {
            Some(parsed) => Self {
                diagnostics: (0..parsed.diagnostics().len()).collect(),
                input: ParsedInput::Native(Arc::new(parsed)),
            },
            None => Self {
                diagnostics: Arc::default(),
                input: ParsedInput::Structured,
            },
        }
    }

    pub(crate) fn syntax(&self) -> Option<ModuleSyntax> {
        match &self.input {
            ParsedInput::Source { physical, key } => Some(
                key.location
                    .resolve(&physical.parsed)
                    .expect("retained module locator"),
            ),
            ParsedInput::Native(parsed) => Some(parsed.module()),
            ParsedInput::Structured => None,
        }
    }

    pub(crate) fn diagnostics(&self) -> impl ExactSizeIterator<Item = &Diagnostic> {
        self.diagnostics.iter().map(|index| {
            &self
                .physical()
                .expect("source diagnostic owner")
                .diagnostics()[*index]
        })
    }

    pub(crate) fn physical(&self) -> Option<&Parse> {
        self.shared_parse().map(Arc::as_ref)
    }

    pub(crate) fn physical_diagnostics(&self) -> &[Diagnostic] {
        self.physical()
            .map_or(&[], |parsed| parsed.diagnostics().as_slice())
    }

    fn shared_parse(&self) -> Option<&Arc<Parse>> {
        match &self.input {
            ParsedInput::Source { physical, .. } => Some(&physical.parsed),
            ParsedInput::Native(parsed) => Some(parsed),
            ParsedInput::Structured => None,
        }
    }

    fn source_parse(&self) -> Option<&PhysicalParse> {
        match &self.input {
            ParsedInput::Source { physical, .. } => Some(physical),
            _ => None,
        }
    }

    fn lowering_key(&self) -> Option<&LoweringInputKey> {
        match &self.input {
            ParsedInput::Source { key, .. } => Some(key),
            _ => None,
        }
    }
}

/// One owned logical source entry, independent of any HIR lowering or arena.
pub(super) struct PreparedSyntax {
    pub(super) source: Arc<SourceFile>,
    pub(super) parsed: ParsedModule,
}

pub(super) struct PreparedSource {
    pub(super) parsed: ParsedModule,
    pub(super) lowered: Arc<LoweredModule>,
}

impl PreparedSyntax {
    /// Only reuse an exact input, and apply its sidecar before sharing a fresh lowering.
    pub(super) fn lower(
        mut self,
        previous: Option<&DeclarationCacheEntry>,
        attachments: Option<LibraryLowering<'_>>,
        cancel: &CancellationToken,
    ) -> Result<PreparedSource, AnalysisError> {
        cancel.check()?;
        let ParsedInput::Source { key, .. } = &mut self.parsed.input else {
            unreachable!("source preparation produces source views only");
        };
        key.attachments = attachments.as_ref().map(|input| input.key().clone());
        let retained = previous
            .filter(|old| old.parsed.lowering_key() == Some(key))
            .and_then(|old| old.lowered.upgrade());
        let lowered = if let Some(retained) = retained {
            retained
        } else {
            let syntax = self.parsed.syntax().expect("prepared source syntax");
            let mut lowered = lower_module_controlled(self.source, &syntax, cancel);
            if let Some(attachments) = attachments {
                attachments.attach(&mut lowered, cancel)?;
            }
            Arc::new(lowered)
        };
        cancel.check()?;
        Ok(PreparedSource {
            parsed: self.parsed,
            lowered,
        })
    }
}

struct LogicalSource {
    source: Arc<SourceFile>,
    syntax: ModuleSyntax,
    diagnostics: Vec<usize>,
}

pub(super) fn prepare(
    mut preparer: SourcePreparer<'_>,
    sources: &SourceSnapshot,
    previous: Option<&DeclarationCache>,
    cancel: &CancellationToken,
) -> Result<BTreeMap<FileId, PreparedSource>, AnalysisError> {
    let prior = |id| previous.and_then(|cache| cache.files.get(&id));
    let syntax = prepare_syntax(
        &mut preparer,
        sources,
        |id| prior(id).map(|old| &old.parsed),
        cancel,
    )?;
    let files = syntax
        .into_iter()
        .map(|(id, input)| Ok((id, input.lower(prior(id), None, cancel)?)))
        .collect::<Result<_, AnalysisError>>()?;
    cancel.check()?;
    Ok(files)
}

/// Prepares an owned manifest of supplied sources and their discovered inline modules.
///
/// Physical parses are shared; logical sources retain original byte coordinates and
/// their assigned parser diagnostics. No HIR is constructed or retained. Only a
/// complete, cancellation-checked syntax batch publishes the source parse cache;
/// subsequent lowering failures do not invalidate these completed syntax results.
/// `published` supplies only the earlier syntax result for each requested physical file.
/// Its exact source/limit matches take precedence over the
/// latest parse cache to preserve published syntax identity after semantic cancellation.
/// No previous entries are added to the cache unless present in this input batch.
/// Native declarations remain on the separate native import path.
///
/// Returns an error for cancellation or an invalid supplied inline range, without
/// publishing the staged parses or returning a partial manifest.
pub(super) fn prepare_syntax<'a>(
    preparer: &mut SourcePreparer<'_>,
    sources: &SourceSnapshot,
    published: impl Fn(FileId) -> Option<&'a ParsedModule>,
    cancel: &CancellationToken,
) -> Result<BTreeMap<FileId, PreparedSyntax>, AnalysisError> {
    let mut files = BTreeMap::new();
    let mut parses = BTreeMap::new();
    for source in sources.files() {
        cancel.check()?;
        let id = source.id();
        let physical =
            preparer
                .cache
                .get_or_parse(source, preparer.limits, published(id), cancel)?;
        for logical in discover_modules(&physical, preparer.inline_ids, cancel)? {
            cancel.check()?;
            let file = logical.source.id();
            let key = LoweringInputKey {
                file,
                location: logical.syntax.location(),
                attachments: None,
            };
            let parsed = ParsedModule {
                input: ParsedInput::Source {
                    physical: physical.clone(),
                    key,
                },
                diagnostics: logical.diagnostics.into(),
            };
            let prepared = PreparedSyntax {
                source: logical.source,
                parsed,
            };
            files.insert(file, prepared);
        }
        parses.insert(id, physical);
    }
    preparer.cache.publish(sources.revision(), parses, cancel)?;
    Ok(files)
}

/// Enumerates logical modules and assigns diagnostics without consulting either cache.
fn discover_modules(
    physical: &PhysicalParse,
    inline_ids: &mut HashMap<(FileId, String), FileId>,
    cancel: &CancellationToken,
) -> Result<Vec<LogicalSource>, AnalysisError> {
    let source = &physical.source;
    let parsed = &physical.parsed;
    let root =
        supplied_module_syntax(parsed).ok_or(AnalysisError::InvalidSourceRange(source.id()))?;
    let mut pending = VecDeque::from([(source.clone(), root)]);
    let mut generated = HashSet::default();
    let mut logical = Vec::new();
    while let Some((source, syntax)) = pending.pop_front() {
        cancel.check()?;
        for item in syntax.items() {
            cancel.check()?;
            let ItemView::ModuleDef(module) = item else {
                continue;
            };
            let (Some(name), Some(child)) = (module.name_text(), module.body()) else {
                continue;
            };
            let Some(range) = child.content_range() else {
                continue;
            };
            if !generated.insert((source.id(), name.clone())) {
                continue;
            }
            let id = *inline_ids
                .entry((source.id(), name.clone()))
                .or_insert_with(FileId::fresh);
            pending.push_back((
                Arc::new(SourceFile::inline_module(&source, &name, id, range)),
                child,
            ));
        }
        logical.push(LogicalSource {
            source,
            syntax,
            diagnostics: Vec::new(),
        });
    }
    distribute_diagnostics(parsed, &mut logical, cancel)?;
    Ok(logical)
}

fn distribute_diagnostics(
    parsed: &Parse,
    sources: &mut [LogicalSource],
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    for (index, diagnostic) in parsed.diagnostics().iter().enumerate() {
        cancel.check()?;
        let mut owner = 0;
        let mut size = usize::MAX;
        if !matches!(diagnostic.kind, DiagnosticKind::CompileLimitExceeded { .. })
            && let Some(span) = diagnostic.span
        {
            for (candidate, source) in sources.iter().enumerate().skip(1) {
                cancel.check()?;
                let range = source.source.inline_range().expect("inline source");
                // An end-delimiter failure can close several nested recovered blocks.
                // The flat parser diagnostics cannot disambiguate that ownership.
                if matches!(diagnostic.kind, DiagnosticKind::ExpectedBlockEnd)
                    && span.start == range.end
                {
                    owner = 0;
                    break;
                }
                if range.start <= span.start
                    && span.end <= range.end
                    && range.end - range.start < size
                {
                    owner = candidate;
                    size = range.end - range.start;
                }
            }
        }
        sources[owner].diagnostics.push(index);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
