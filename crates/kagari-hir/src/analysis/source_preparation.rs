//! Physical parse retention and logical inline source preparation.

use crate::{
    analysis::{AnalysisDatabase, declaration_queries::DeclarationSnapshot, error::AnalysisError},
    lower::{LoweredModule, lower_module_controlled},
};
use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    identity::FileId,
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_syntax::{
    ast::{
        item::Item,
        module::{ModuleLocation, ModuleSyntax},
    },
    parser::{Parse, parse_with_limits},
};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    sync::Arc,
};

/// A logical view paired with the exact immutable parse that owns its locator.
#[derive(Debug, Clone)]
pub(crate) struct ParsedModule {
    physical: Arc<Parse>,
    location: ModuleLocation,
    diagnostics: Arc<[usize]>,
}

impl ParsedModule {
    pub(crate) fn root(parsed: Parse) -> Self {
        Self {
            location: ModuleSyntax::from(parsed.syntax()).location(),
            diagnostics: (0..parsed.diagnostics().len()).collect(),
            physical: Arc::new(parsed),
        }
    }

    pub(crate) fn syntax(&self) -> ModuleSyntax {
        self.location
            .resolve(&self.physical.syntax())
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
    let mut files = BTreeMap::new();
    for physical in sources.files() {
        cancel.check()?;
        let old = previous
            .and_then(|old| old.file(physical.id()))
            .filter(|old| old.source().revision() == physical.revision());
        let parsed = match old {
            Some(old) => old.parsed.physical.clone(),
            None => Arc::new(parse_with_limits(physical, db.parse_limits, cancel)?),
        };
        let syntax = match physical.inline_range() {
            Some(range) => ModuleSyntax::at_content_range(&parsed.syntax(), range)
                .ok_or(AnalysisError::InvalidSourceRange(physical.id()))?,
            None => ModuleSyntax::from(parsed.syntax()),
        };
        let mut pending = VecDeque::from([(physical.clone(), syntax)]);
        let mut generated = HashSet::new();
        let mut logical = Vec::new();
        while let Some((source, syntax)) = pending.pop_front() {
            cancel.check()?;
            for item in syntax.items() {
                cancel.check()?;
                let Item::ModuleDef(module) = item else {
                    continue;
                };
                let (Some(name), Some(block)) = (module.name_text(), module.block()) else {
                    continue;
                };
                let child = ModuleSyntax::from(block);
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
            let source = logical.source;
            let old = previous
                .and_then(|old| old.file(source.id()))
                .filter(|old| {
                    old.source().revision() == source.revision()
                        && Arc::ptr_eq(&old.parsed.physical, &parsed)
                });
            let lowered = old.map_or_else(
                || {
                    Arc::new(lower_module_controlled(
                        source.clone(),
                        &logical.syntax,
                        cancel,
                    ))
                },
                |old| old.declared.lowered.clone(),
            );
            files.insert(
                source.id(),
                PreparedSource {
                    parsed: ParsedModule {
                        physical: parsed.clone(),
                        location: logical.syntax.location(),
                        diagnostics: logical.diagnostics.into(),
                    },
                    lowered,
                },
            );
        }
    }
    cancel.check()?;
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
