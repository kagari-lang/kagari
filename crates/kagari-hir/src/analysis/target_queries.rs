//! Capture type-path provenance after checked type targets and arena remapping.
use crate::{
    AnalyzedModule,
    declarations::DeclarationId,
    hir::ty::TypeKind,
    imports::{ResolvedTarget, SourceItem, catalog::LookupHit},
    typeck::table::TypeTarget,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::table::DefinitionId,
    span::Span,
};
use kagari_types::declaration::names::NameNamespace;

pub(super) fn type_hits(
    facts: &AnalyzedModule<DefinitionId>,
    cancel: &CancellationToken,
) -> Result<Vec<(Span, LookupHit)>, Cancelled> {
    let mut hits = Vec::new();
    for (index, (_, reference)) in facts.lowered.module.body.types.iter().enumerate() {
        let id = facts.lowered.source_map.type_id(index);
        cancel.check()?;
        let (TypeKind::Named(name) | TypeKind::Generic { name, .. }) = &reference.kind else {
            continue;
        };
        let Some(target) = facts
            .typed
            .type_table
            .type_ref(id)
            .and_then(|ty| ty.target.as_ref())
        else {
            continue;
        };
        let Some(hit) = facts.names.catalog.resolve_name(
            &facts.names.items,
            &facts.names.hosts,
            name,
            NameNamespace::Type,
            cancel,
        ) else {
            continue;
        };
        let admitted = match (target, &hit.target) {
            (TypeTarget::Source(expected), ResolvedTarget::Source(_)) => facts
                .declarations
                .imported_types()
                .resolved(hit.target.resolved(facts.names.items.unit.as_ref()))
                .is_some_and(|ty| ty.declaration.id == DeclarationId::Definition(*expected)),
            (TypeTarget::Host(expected), ResolvedTarget::HostType(actual)) => expected == actual,
            (TypeTarget::Struct(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Struct(actual) if actual == *expected)
                    && source.unit.matches(&facts.lowered)
            }
            (TypeTarget::Enum(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Enum(actual) if actual == *expected)
                    && source.unit.matches(&facts.lowered)
            }
            (TypeTarget::Trait(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Trait(actual) if actual == *expected)
                    && source.unit.matches(&facts.lowered)
            }
            (TypeTarget::OpaqueType(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::OpaqueType(actual) if actual == *expected)
                    && source.unit.matches(&facts.lowered)
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
                &facts.names.hosts,
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
