//! Capture type-path provenance after checked type targets and arena remapping.
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    span::Span,
};
use kagari_hir::{
    AnalyzedModule,
    analysis::FileAnalysis,
    declarations::DeclarationId,
    hir::ty::TypeKind,
    imports::{BindingOrigin, ResolvedTarget, SourceItem, catalog::LookupHit},
    typeck::table::TypeTarget,
};
use kagari_types::declaration::names::NameNamespace;

fn type_hits(
    facts: &AnalyzedModule,
    cancel: &CancellationToken,
) -> Result<Vec<(Span, LookupHit)>, Cancelled> {
    let mut hits = Vec::new();
    for (id, reference) in facts.lowered.module.body.types() {
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
            (TypeTarget::Source(expected), ResolvedTarget::Source(_)) => facts
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
            (TypeTarget::Struct(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Struct(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, facts.names.catalog.definitions())
            }
            (TypeTarget::Enum(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Enum(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, facts.names.catalog.definitions())
            }
            (TypeTarget::Trait(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Trait(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, facts.names.catalog.definitions())
            }
            (TypeTarget::OpaqueType(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::OpaqueType(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, facts.names.catalog.definitions())
            }
            _ => false,
        };
        if !admitted {
            continue;
        }
        for (prefix, span) in facts.lowered.source_map.type_path(id) {
            cancel.check()?;
            if let Some(hit) = facts.names.catalog.resolve_name(
                &facts.names.items,
                prefix,
                NameNamespace::Type,
                cancel,
            ) {
                hits.push((*span, hit));
            }
        }
    }
    Ok(hits)
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

/// Type-path navigation derived only on demand from one exact semantic owner.
#[derive(Debug)]
pub struct NavigationIndex<'a> {
    analysis: &'a FileAnalysis,
    type_path_hits: Vec<(Span, LookupHit)>,
}

impl<'a> NavigationIndex<'a> {
    /// Builds navigation from settled checked targets; cancellation publishes no index.
    pub fn build(
        analysis: &'a FileAnalysis,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        cancel.check()?;
        let type_path_hits = type_hits(analysis.result().facts(), cancel)?;
        cancel.check()?;
        Ok(Self {
            analysis,
            type_path_hits,
        })
    }

    /// The exact semantic result that owns the indexed nodes and definition IDs.
    pub fn analysis(&self) -> &'a FileAnalysis {
        self.analysis
    }

    /// Temporary prefix provenance until HR07 derives the remaining path indexes.
    pub fn type_path_hits(&self) -> &[(Span, LookupHit)] {
        &self.type_path_hits
    }
}
