//! Physical parse retention and logical inline source preparation.

use crate::{
    analysis::error::AnalysisError,
    collections::{HashMap, HashSet},
    lower::{LoweredModule, lower_module_controlled, supplied_module_syntax},
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
        module::{ModuleLocation, ModuleSyntax},
        views::ItemView,
    },
    parser::{Parse, ParseLimits, ParseOptions, parse},
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

/// Source limits and the identity allocator for logical inline modules.
pub(super) struct SourcePreparer<'a> {
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
        physical: Arc<Parse>,
        location: ModuleLocation,
    },
    Structured,
}

impl ParsedModule {
    pub(crate) fn root(parsed: Parse) -> Self {
        Self {
            diagnostics: (0..parsed.diagnostics().len()).collect(),
            input: ParsedInput::Source {
                location: parsed.module().location(),
                physical: Arc::new(parsed),
            },
        }
    }

    pub(super) fn structured() -> Self {
        Self {
            diagnostics: Arc::default(),
            input: ParsedInput::Structured,
        }
    }

    pub(crate) fn syntax(&self) -> Option<ModuleSyntax> {
        match &self.input {
            ParsedInput::Source { physical, location } => {
                Some(location.resolve(physical).expect("retained module locator"))
            }
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
            ParsedInput::Source { physical, .. } => Some(physical),
            ParsedInput::Structured => None,
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
    /// Lowers this logical view from its physical parse.
    fn lower(self, cancel: &CancellationToken) -> Result<PreparedSource, AnalysisError> {
        cancel.check()?;
        let syntax = self.parsed.syntax().expect("prepared source syntax");
        let lowered = lower_module_controlled(self.source, &syntax, cancel);
        cancel.check()?;
        Ok(PreparedSource {
            parsed: self.parsed,
            lowered: Arc::new(lowered),
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
    cancel: &CancellationToken,
) -> Result<BTreeMap<FileId, PreparedSource>, AnalysisError> {
    let syntax = prepare_syntax(&mut preparer, sources, cancel)?;
    let files = syntax
        .into_iter()
        .map(|(id, input)| Ok((id, input.lower(cancel)?)))
        .collect::<Result<_, AnalysisError>>()?;
    cancel.check()?;
    Ok(files)
}

/// Prepares an owned manifest of supplied sources and their discovered inline modules.
///
/// Physical parses are shared; logical sources retain original byte coordinates and
/// their assigned parser diagnostics. No HIR is constructed. Native declarations
/// remain on the separate registration input path.
///
/// Returns an error for cancellation or an invalid supplied inline range, without
/// returning a partial manifest.
pub(super) fn prepare_syntax(
    preparer: &mut SourcePreparer<'_>,
    sources: &SourceSnapshot,
    cancel: &CancellationToken,
) -> Result<BTreeMap<FileId, PreparedSyntax>, AnalysisError> {
    let mut files = BTreeMap::new();
    for source in sources.files() {
        cancel.check()?;
        let options = ParseOptions {
            limits: preparer.limits,
        };
        let physical = Arc::new(parse(source, options, cancel)?);
        for logical in discover_modules(source, &physical, preparer.inline_ids, cancel)? {
            cancel.check()?;
            let file = logical.source.id();
            let parsed = ParsedModule {
                input: ParsedInput::Source {
                    physical: physical.clone(),
                    location: logical.syntax.location(),
                },
                diagnostics: logical.diagnostics.into(),
            };
            let prepared = PreparedSyntax {
                source: logical.source,
                parsed,
            };
            files.insert(file, prepared);
        }
    }
    cancel.check()?;
    Ok(files)
}

/// Enumerates logical modules from one parse and assigns physical diagnostics.
fn discover_modules(
    source: &Arc<SourceFile>,
    parsed: &Parse,
    inline_ids: &mut HashMap<(FileId, String), FileId>,
    cancel: &CancellationToken,
) -> Result<Vec<LogicalSource>, AnalysisError> {
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
