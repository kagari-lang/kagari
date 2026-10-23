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
    span::Span,
};
use kagari_types::declaration::names::NameNamespace;

pub(super) fn type_hits(
    facts: &AnalyzedModule,
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
                .target(hit.target.resolved(facts.names.items.unit.as_ref()))
                .or_else(|| {
                    facts
                        .declarations
                        .imported_types()
                        .resolved(hit.target.resolved(facts.names.items.unit.as_ref()))
                        .map(|ty| &ty.declaration)
                })
                .is_some_and(|declaration| declaration.id == DeclarationId::Definition(*expected)),
            (TypeTarget::Struct(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Struct(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, &facts.names.catalog.definitions)
            }
            (TypeTarget::Enum(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Enum(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, &facts.names.catalog.definitions)
            }
            (TypeTarget::Trait(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::Trait(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, &facts.names.catalog.definitions)
            }
            (TypeTarget::OpaqueType(expected), ResolvedTarget::Source(source)) => {
                matches!(source.item, SourceItem::OpaqueType(actual) if actual == *expected)
                    && source
                        .unit
                        .matches(&facts.lowered, &facts.names.catalog.definitions)
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
