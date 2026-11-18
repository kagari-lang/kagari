//! Demand-built path provenance after semantic settlement and fact remapping.

use crate::sites::SourceSites;
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    span::Span,
};
use kagari_hir::{
    AnalyzedModule,
    analysis::FileAnalysis,
    declarations::DeclarationId,
    hir::{expr::ExprKind, ty::TypeKind},
    imports::{
        BindingOrigin, DirectiveResolution, ImportKind, ResolvedTarget, SourceItem,
        bindings::LookupOutcome,
        catalog::{LookupContext, LookupHit, LookupResult},
    },
    typeck::table::ResolvedTypeTarget,
};
use kagari_types::declaration::names::NameNamespace;

fn prefixes(path: &[(String, Span)]) -> impl Iterator<Item = (String, Span)> + '_ {
    let mut prefix = String::new();
    path.iter().map(move |(name, span)| {
        if !prefix.is_empty() {
            prefix.push_str("::");
        }
        prefix.push_str(name);
        (prefix.clone(), *span)
    })
}

fn type_hits(
    facts: &AnalyzedModule,
    sites: &SourceSites<'_>,
    cancel: &CancellationToken,
) -> Result<Vec<(Span, LookupHit)>, Cancelled> {
    let mut hits = Vec::new();
    for (id, reference) in facts.lowered.module.nodes.types() {
        cancel.check()?;
        let (TypeKind::Named(name) | TypeKind::Generic { name, .. }) = &reference.kind else {
            continue;
        };
        let Some(target) = facts
            .typed
            .type_table()
            .type_ref(id)
            .and_then(|ty| ty.target.as_ref())
        else {
            continue;
        };
        let Some(hit) =
            facts
                .names
                .catalog
                .resolve_name(&facts.names.items, name, NameNamespace::Type, cancel)
        else {
            continue;
        };
        let admitted = match (target, &hit.target) {
            (ResolvedTypeTarget::Source(expected), ResolvedTarget::Source(_)) => facts
                .declarations
                .target(hit.target.resolved(facts.names.items.source_unit()))
                .or_else(|| {
                    facts
                        .declarations
                        .imported_types()
                        .resolved(hit.target.resolved(facts.names.items.source_unit()))
                        .map(|ty| &ty.declaration)
                })
                .is_some_and(|declaration| declaration.id == DeclarationId::Definition(*expected)),
            (ResolvedTypeTarget::Struct(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Struct(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, facts.names.catalog.definitions())
            }
            (ResolvedTypeTarget::Enum(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Enum(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, facts.names.catalog.definitions())
            }
            (ResolvedTypeTarget::Trait(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Trait(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, facts.names.catalog.definitions())
            }
            (ResolvedTypeTarget::NativeType(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::NativeType(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, facts.names.catalog.definitions())
            }
            _ => false,
        };
        if !admitted {
            continue;
        }
        for (prefix, span) in prefixes(sites.type_path(id)) {
            cancel.check()?;
            if let Some(hit) = facts.names.catalog.resolve_name(
                &facts.names.items,
                &prefix,
                NameNamespace::Type,
                cancel,
            ) {
                hits.push((span, hit));
            }
        }
    }
    Ok(hits)
}

fn expression_hits(
    facts: &AnalyzedModule,
    sites: &SourceSites<'_>,
    hits: &mut Vec<(Span, LookupHit)>,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    let names = &facts.names;
    for (id, expr) in facts.lowered.module.nodes.expressions() {
        cancel.check()?;
        let ExprKind::Name { name, .. } = &expr.kind else {
            continue;
        };
        let lookup = |name: &str, namespace| {
            names
                .catalog
                .resolve_name(&names.items, name, namespace, cancel)
        };
        let selected = names.expr_resolution(id);
        let full = lookup(name, NameNamespace::Value).filter(|hit| {
            selected.as_ref() == Some(&hit.target.resolved(names.items.source_unit()))
        });
        // A lexical binding must not acquire module provenance just because the
        // catalog also contains that spelling (including hoisted local functions).
        if selected.is_some() && full.is_none() {
            continue;
        }
        if let Some(member) = names.qualified_member(id)
            && let Some((owner, _)) = name.rsplit_once("::")
            && !lookup(owner, NameNamespace::Type)
                .is_some_and(|hit| hit.target.resolved(names.items.source_unit()) == member.owner)
        {
            continue;
        }
        for (prefix, span) in prefixes(sites.expr_path(id)) {
            cancel.check()?;
            let hit = if &prefix == name {
                full.clone()
            } else {
                lookup(&prefix, NameNamespace::Type)
            };
            if let Some(hit) = hit {
                hits.push((span, hit));
            }
        }
        // Synthetic shorthand name expressions have no PathExpr syntax.
        if sites.expr_path(id).is_empty()
            && let Some(hit) = full
            && let Some(span) = sites
                .expr_reference_span(id)
                .or_else(|| facts.lowered.module.expr(id).origin.source_span())
        {
            hits.push((span, hit));
        }
    }
    Ok(())
}

fn import_hits(
    facts: &AnalyzedModule,
    sites: &SourceSites<'_>,
    hits: &mut Vec<(Span, LookupHit)>,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    let names = &facts.names;
    for (slot, directive) in names.imports.directives.iter().enumerate() {
        cancel.check()?;
        let Some(path_sites) = sites.imports.get(slot) else {
            continue;
        };
        for (end, span) in path_sites {
            cancel.check()?;
            let prefix = &directive.segments[..*end];
            let terminal = *end == directive.segments.len();
            for namespace in NameNamespace::ALL {
                if namespace == NameNamespace::Value
                    && (!terminal || matches!(directive.kind, ImportKind::Glob))
                {
                    continue;
                }
                let selected = match &directive.resolution {
                    DirectiveResolution::Named(outcomes) => &outcomes[namespace],
                    DirectiveResolution::Glob(outcome) => outcome,
                };
                if *selected == LookupOutcome::Ambiguous {
                    continue;
                }
                let excluded = if matches!(directive.kind, ImportKind::Glob) {
                    NameNamespace::Type
                } else {
                    namespace
                };
                let ctx = LookupContext::new(&directive.id.unit.module)
                    .excluding(directive.id.clone(), excluded);
                let LookupResult::Found(mut hit) =
                    names.catalog.absolute(&ctx, prefix, namespace, cancel)?
                else {
                    continue;
                };
                // Terminal provenance must agree with settled category selection;
                // failed terminals may still have useful, resolved Type prefixes.
                if terminal && selected.target() != Some(&hit.target) {
                    continue;
                }
                hit.via.insert(
                    0,
                    match directive.kind {
                        ImportKind::Named { .. } => {
                            BindingOrigin::NamedImport(directive.id.clone())
                        }
                        ImportKind::Glob => BindingOrigin::GlobImport(directive.id.clone()),
                    },
                );
                hits.push((*span, hit));
            }
        }
    }
    Ok(())
}

/// A selected navigation category and its origins, without import-solver work state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationTarget {
    /// Type or Value category selected by semantic resolution.
    pub namespace: NameNamespace,
    /// Canonical destination in the retained semantic snapshot.
    pub target: ResolvedTarget,
    /// Import origins in traversal order, including equal-target glob contributions.
    pub via: Vec<BindingOrigin>,
}

impl From<&LookupHit> for NavigationTarget {
    fn from(hit: &LookupHit) -> Self {
        Self {
            namespace: hit.namespace,
            target: hit.target.clone(),
            via: hit.via.clone(),
        }
    }
}

/// Path navigation derived only on demand from one exact settled semantic owner.
#[derive(Debug)]
pub struct NavigationIndex<'a> {
    analysis: &'a FileAnalysis,
    hits: Vec<(Span, LookupHit)>,
}

impl<'a> NavigationIndex<'a> {
    /// Builds navigation from settled checked targets; cancellation publishes no index.
    pub fn build(
        analysis: &'a FileAnalysis,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        cancel.check()?;
        let facts = analysis.result().facts();
        if analysis.syntax().is_none() {
            return Ok(Self {
                analysis,
                hits: vec![],
            });
        }
        let sites = SourceSites::build(&facts.lowered, &analysis.syntax(), cancel)?;
        let mut hits = type_hits(facts, &sites, cancel)?;
        expression_hits(facts, &sites, &mut hits, cancel)?;
        import_hits(facts, &sites, &mut hits, cancel)?;
        cancel.check()?;
        Ok(Self { analysis, hits })
    }

    /// The exact semantic result that owns the indexed nodes and definition IDs.
    pub fn analysis(&self) -> &'a FileAnalysis {
        self.analysis
    }

    pub(crate) fn hits(&self) -> &[(Span, LookupHit)] {
        &self.hits
    }
}
