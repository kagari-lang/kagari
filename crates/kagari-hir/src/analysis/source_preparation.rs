//! Physical parse retention and logical inline source preparation.

use crate::{
    analysis::{AnalysisDatabase, declarations::DeclarationSnapshot, error::AnalysisError},
    lower::{LoweredModule, lower_module_controlled},
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
    collections::{BTreeMap, HashSet, VecDeque},
    sync::Arc,
};

/// Database-owned physical parses, independent of declaration and lowering owners.
///
/// The database stores this behind a `RefCell` because preparation takes `&self`.
/// Cloning forks the index while sharing immutable source/parse allocations. Only a
/// complete source-syntax batch may replace it; older snapshots cannot evict
/// newer entries. Parser-limit setters must call [`Self::clear`]. Native declaration
/// parses remain owned by the existing native import path, not this cache.
#[derive(Debug, Clone, Default)]
pub(super) struct SourceParseCache {
    revision: Revision,
    limits: Option<ParseLimits>,
    files: BTreeMap<FileId, CachedParse>,
}

#[derive(Debug, Clone)]
struct CachedParse {
    source: Arc<SourceFile>,
    parsed: Arc<Parse>,
}

impl SourceParseCache {
    /// Invalidates parses and the publication watermark after parser inputs change.
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    fn get(&self, source: &Arc<SourceFile>, limits: ParseLimits) -> Option<Arc<Parse>> {
        if self.limits != Some(limits) {
            return None;
        }
        self.files
            .get(&source.id())
            // SourceFile can be cloned and given different copied-source metadata
            // without changing its ID/revision. Only the exact immutable input is
            // sufficient evidence; equal text or revision alone is not.
            .filter(|entry| Arc::ptr_eq(&entry.source, source))
            .map(|entry| entry.parsed.clone())
    }

    fn publish(
        &mut self,
        revision: Revision,
        limits: ParseLimits,
        files: BTreeMap<FileId, CachedParse>,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        cancel.check()?;
        if revision >= self.revision {
            self.revision = revision;
            self.limits = Some(limits);
            self.files = files;
        }
        Ok(())
    }
}

/// A logical view paired with the exact immutable parse that owns its locator.
#[derive(Debug, Clone)]
pub(crate) struct ParsedModule {
    physical: Arc<Parse>,
    location: ModuleLocation,
    diagnostics: Arc<[usize]>,
    // Native root parses are prepared separately and are not source-reuse inputs.
    limits: Option<ParseLimits>,
}

impl ParsedModule {
    pub(crate) fn root(parsed: Parse) -> Self {
        Self {
            location: parsed.module().location(),
            diagnostics: (0..parsed.diagnostics().len()).collect(),
            physical: Arc::new(parsed),
            limits: None,
        }
    }

    pub(crate) fn syntax(&self) -> ModuleSyntax {
        self.location
            .resolve(&self.physical)
            .expect("retained module locator")
    }

    pub(crate) fn diagnostics(&self) -> impl ExactSizeIterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .map(|index| &self.physical.diagnostics()[*index])
    }

    pub(crate) fn physical(&self) -> &Parse {
        &self.physical
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

struct LogicalSource {
    source: Arc<SourceFile>,
    syntax: ModuleSyntax,
    diagnostics: Vec<usize>,
}

pub(super) fn prepare(
    db: &AnalysisDatabase,
    sources: &SourceSnapshot,
    previous: Option<&DeclarationSnapshot>,
    cancel: &CancellationToken,
) -> Result<BTreeMap<FileId, PreparedSource>, AnalysisError> {
    // Syntax publication may advance ahead of semantic publication. Retain only
    // the requested published syntax inputs so a cancelled newer analysis cannot
    // force an unchanged published lowering to acquire a new parse identity.
    let mut retained = BTreeMap::new();
    for source in sources.files() {
        cancel.check()?;
        if let Some(old) = previous.and_then(|old| old.file(source.id())) {
            retained.insert(
                source.id(),
                PreparedSyntax {
                    source: old.declared.lowered.source.clone(),
                    parsed: old.parsed.clone(),
                },
            );
        }
    }
    let syntax = prepare_syntax(db, sources, Some(&retained), cancel)?;
    let mut files = BTreeMap::new();
    for (id, PreparedSyntax { source, parsed }) in syntax {
        cancel.check()?;
        let old = previous.and_then(|old| old.file(id)).filter(|old| {
            old.source().revision() == source.revision()
                && Arc::ptr_eq(&old.parsed.physical, &parsed.physical)
        });
        let lowered = old.map_or_else(
            || Arc::new(lower_module_controlled(source, &parsed.syntax(), cancel)),
            |old| old.declared.lowered.clone(),
        );
        files.insert(id, PreparedSource { parsed, lowered });
    }
    cancel.check()?;
    Ok(files)
}

/// Prepares an owned manifest of supplied sources and their discovered inline modules.
///
/// Physical parses are shared; logical sources retain original byte coordinates and
/// their assigned parser diagnostics. No HIR is constructed or retained. Only a
/// complete, cancellation-checked syntax batch publishes the source parse cache;
/// subsequent lowering failures do not invalidate these completed syntax results.
/// `previous` is an optional owned syntax manifest borrowed for this request, not
/// a semantic snapshot. Its exact source/limit matches take precedence over the
/// latest parse cache to preserve published syntax identity after semantic cancellation.
/// No previous entries are added to the cache unless present in this input batch.
/// Native declarations remain on the separate native import path.
///
/// Returns an error for cancellation or an invalid supplied inline range, without
/// publishing the staged parses or returning a partial manifest.
pub(super) fn prepare_syntax(
    db: &AnalysisDatabase,
    sources: &SourceSnapshot,
    previous: Option<&BTreeMap<FileId, PreparedSyntax>>,
    cancel: &CancellationToken,
) -> Result<BTreeMap<FileId, PreparedSyntax>, AnalysisError> {
    let mut files = BTreeMap::new();
    let mut parses = BTreeMap::new();
    for physical in sources.files() {
        cancel.check()?;
        let cached = previous
            .and_then(|files| files.get(&physical.id()))
            .filter(|old| {
                Arc::ptr_eq(&old.source, physical) && old.parsed.limits == Some(db.parse_limits)
            })
            .map(|old| old.parsed.physical.clone())
            .or_else(|| {
                db.source_parse_cache
                    .borrow()
                    .get(physical, db.parse_limits)
            });
        let parsed = match cached {
            Some(parsed) => parsed,
            None => Arc::new(parse(
                physical,
                ParseOptions {
                    limits: db.parse_limits,
                    ..Default::default()
                },
                cancel,
            )?),
        };
        parses.insert(
            physical.id(),
            CachedParse {
                source: physical.clone(),
                parsed: parsed.clone(),
            },
        );
        let syntax = match physical.inline_range() {
            Some(range) => ModuleSyntax::at_content_range(&parsed, range)
                .ok_or(AnalysisError::InvalidSourceRange(physical.id()))?,
            None => parsed.module(),
        };
        let mut pending = VecDeque::from([(physical.clone(), syntax)]);
        let mut generated = HashSet::new();
        let mut logical = Vec::new();
        while let Some((source, syntax)) = pending.pop_front() {
            cancel.check()?;
            for item in syntax.items() {
                cancel.check()?;
                let ItemView::ModuleDef(module) = item else {
                    continue;
                };
                let (Some(name), Some(block)) = (module.name_text(), module.body()) else {
                    continue;
                };
                let child = block;
                let Some(range) = child.content_range() else {
                    continue;
                };
                if !generated.insert((source.id(), name.clone())) {
                    continue;
                }
                let id = *db
                    .inline_ids
                    .borrow_mut()
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
        distribute_diagnostics(&parsed, &mut logical, cancel)?;
        for logical in logical {
            cancel.check()?;
            files.insert(
                logical.source.id(),
                PreparedSyntax {
                    parsed: ParsedModule {
                        physical: parsed.clone(),
                        location: logical.syntax.location(),
                        diagnostics: logical.diagnostics.into(),
                        limits: Some(db.parse_limits),
                    },
                    source: logical.source,
                },
            );
        }
    }
    db.source_parse_cache.borrow_mut().publish(
        sources.revision(),
        db.parse_limits,
        parses,
        cancel,
    )?;
    Ok(files)
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
