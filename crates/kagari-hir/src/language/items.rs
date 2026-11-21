//! Header-time role collection. Roles identify ordinary traits, not a second trait model.
pub(crate) mod conversion;
mod projection;
mod shapes;
#[cfg(test)]
mod tests;
use crate::aggregates::traits::TraitSignature;
use crate::{
    DiagnosticBuffer,
    aggregates::AggregateCatalog,
    declarations::{DeclarationId, Declarations},
    hir::{
        attribute::{AttributeForm, AttributeTarget, AttributeValue},
        item::Item,
    },
    lower::LoweredModule,
    resolver::resolved::ResolvedName,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::table::{DefinitionId, DefinitionTable},
    span::Span,
};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    literal::decode_string_literal,
};
use kagari_types::{
    declaration::names::NameNamespace,
    language::{self, requirements, role::LangRole, schema::CoreEnum},
    visibility::Visibility,
};
use std::collections::{BTreeMap, btree_map::Entry};

/// Collects canonical `#[lang]` identities from actual declarations.
pub(crate) fn collect(
    lowered: &LoweredModule,
    declarations: &Declarations,
    diagnostics: &mut DiagnosticBuffer,
) -> BTreeMap<LangRole, DefinitionId> {
    let mut items = BTreeMap::new();
    let mut report = |role: &str, reason: &str, span: Span| {
        diagnostics.push(
            Diagnostic::error(DiagnosticKind::InvalidLanguageRole {
                role: role.into(),
                reason: reason.into(),
            })
            .with_span(span),
        )
    };
    for attribute in lowered
        .module
        .attributes
        .iter()
        .filter(|attribute| attribute.name == "lang")
    {
        let AttributeForm::Value(AttributeValue::Literal(value)) = &attribute.form else {
            report("lang", "expected #[lang = \"role\"]", attribute.span);
            continue;
        };
        let Ok(name) = decode_string_literal(value) else {
            report("lang", "role must be a String literal", attribute.span);
            continue;
        };
        let Some(role) = LangRole::parse(&name) else {
            report(&name, "unknown reserved role", attribute.span);
            continue;
        };
        if lowered.source.module_identity() != &language::identity(role.protocol()).module {
            report(
                &name,
                "language roles require their canonical owning module",
                attribute.span,
            );
            continue;
        }
        let AttributeTarget::Item(Item::Trait(id)) = attribute.target else {
            report(&name, "role requires a trait declaration", attribute.span);
            continue;
        };
        let item = &lowered.module.traits[id.index()];
        let Some(declaration) = declarations.target(ResolvedName::Trait(id)) else {
            report(&name, "missing declaration identity", attribute.span);
            continue;
        };
        let DeclarationId::Definition(id) = &declaration.id else {
            unreachable!("trait declaration identity")
        };
        if item.visibility != Visibility::Public
            || declarations
                .definitions()
                .lookup(&language::identity(role.protocol()))
                != Some(*id)
        {
            report(
                &name,
                "role differs from the canonical declaration identity or visibility",
                attribute.span,
            );
            continue;
        }
        match items.entry(role) {
            Entry::Vacant(entry) => {
                entry.insert(*id);
            }
            Entry::Occupied(_) => report(&name, "duplicate role", attribute.span),
        }
    }
    if language::is_language_module(lowered.source.module_identity()) {
        for role in LangRole::ALL.into_iter().filter(|role| {
            language::identity(role.protocol()).module == *lowered.source.module_identity()
        }) {
            if !items.contains_key(&role) {
                report(role.name(), "missing required role", Span::default());
            }
        }
    }
    items
}

/// Check language-required shapes against their semantic requirements, not a copy
/// supplied by the producer. Static conversion relationships use the shared verifier.
pub(crate) fn validate_shapes(
    lowered: &LoweredModule,
    declarations: &Declarations,
    aggregates: &AggregateCatalog,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let definitions = declarations.definitions();
    for required in CoreEnum::COMPARISON {
        let path = required.declaration();
        if &path.module != lowered.source.module_identity() {
            continue;
        }
        let valid = definitions.lookup(&path).is_some_and(|owner| {
            matches!(declarations.definition_target(&owner), Some(ResolvedName::Enum(id)) if lowered.module.enums[id.index()].visibility == Visibility::Public)
                && aggregates.enumeration(&owner)
                    .and_then(|actual| projection::enumeration(actual, definitions))
                    .is_some_and(|record| required.shape_valid(owner, &record, definitions))
        });
        if !valid {
            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidLanguageRole {
                role: path.path[0].name.clone(),
                reason: "missing or invalid public comparison enum".into(),
            }));
        }
    }
    for (role, id) in &declarations.language_items {
        if cancel.check().is_err() {
            return;
        }
        if !aggregates.trait_(id).is_some_and(|actual| {
            shapes::valid(actual, role.protocol(), declarations.definitions())
        }) {
            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidLanguageRole {
                role: role.name().into(),
                reason: "declaration binders, parents or member shapes differ from language requirements".into(),
            }));
        }
    }
    let try_from = conversion::try_from(definitions);
    for item in &lowered.module.traits {
        if cancel.check().is_err() {
            return;
        }
        let Some(actual) = declarations
            .definition(ResolvedName::Trait(item.id))
            .and_then(|id| aggregates.trait_(id))
        else {
            continue;
        };
        let canonical = Some(actual.id) == try_from;
        if !canonical && actual.conversion_adapter.is_none() {
            continue;
        }
        let exposed = match conversion_exposed(actual, declarations, cancel) {
            Ok(exposed) => exposed,
            Err(_) => return,
        };
        let canonical_shape = !canonical
            || (item.visibility == Visibility::Public
                && item
                    .conversion_adapter
                    .as_ref()
                    .is_none_or(|supplied| actual.conversion_adapter.as_ref() == Some(supplied)));
        if !exposed
            || !canonical_shape
            || !conversion_valid(actual, definitions, canonical)
            || !shapes::conversion_targets_valid(actual, aggregates)
        {
            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidLanguageRole {
                role: format!("{:?}", actual.id),
                reason: "invalid static conversion relationship".into(),
            }));
        }
    }
}

fn conversion_exposed(
    actual: &TraitSignature,
    declarations: &Declarations,
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    let Some(adapter) = &actual.conversion_adapter else {
        return Ok(true);
    };
    Ok(declarations
        .resolve_definition_exposure(adapter.result, NameNamespace::Type, None, cancel)?
        .is_some())
}

fn conversion_valid(
    actual: &TraitSignature,
    definitions: &DefinitionTable,
    canonical: bool,
) -> bool {
    projection::simple_trait(actual, definitions).is_some_and(|record| {
        if canonical {
            requirements::trait_shape_valid(actual.id, &record, definitions)
        } else {
            record.supertraits.is_empty()
                && record
                    .conversion_adapter
                    .as_ref()
                    .is_some_and(|adapter| adapter.valid_in(&actual.id, &record, definitions))
        }
    })
}
