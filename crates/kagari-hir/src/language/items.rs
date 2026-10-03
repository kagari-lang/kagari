//! Header-time role collection. Roles identify ordinary traits, not a second trait model.
use kagari_contract::library::catalog;
#[cfg(test)]
mod tests;
use crate::{
    DiagnosticBuffer,
    aggregates::AggregateCatalog,
    declarations::{DeclarationId, Declarations},
    hir::item::{Item, storage::Visibility},
    lower::{AttributeValue, LoweredModule},
    resolver::resolved::ResolvedName,
    typeck::table::ConstraintTarget,
    types::abi::{lower_nominal_type, lower_type},
};
use kagari_contract::{
    language::{self, role::LangRole},
    types::Constraint,
};
use std::collections::{BTreeMap, btree_map::Entry};
use {
    kagari_common::{identity::DefinitionPath, span::Span},
    kagari_source::{
        diagnostic::{Diagnostic, DiagnosticKind},
        literal::decode_string_literal,
    },
};

pub(crate) fn collect(
    lowered: &LoweredModule,
    declarations: &Declarations,
    diagnostics: &mut DiagnosticBuffer,
) -> BTreeMap<LangRole, DefinitionPath> {
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
        .attributes
        .iter()
        .filter(|attribute| attribute.name == "lang")
    {
        let Some(AttributeValue::Literal(value)) = &attribute.value else {
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
        if !lowered.language_foundation
            || lowered.source.module_identity() != &language::module_identity()
        {
            report(
                &name,
                "only installed language foundation source may declare roles",
                attribute.span,
            );
            continue;
        }
        let Some(item) = lowered.module.traits.iter().find(|item| {
            lowered.source_map.item_span(Item::Trait(item.id)) == attribute.target_span
        }) else {
            report(&name, "role requires a trait declaration", attribute.span);
            continue;
        };
        let Some(declaration) = declarations.target(ResolvedName::Trait(item.id)) else {
            report(&name, "missing declaration identity", attribute.span);
            continue;
        };
        let DeclarationId::Definition(id) = &declaration.id else {
            unreachable!("trait declaration identity")
        };
        if item.visibility != Visibility::Public || *id != language::identity(role.protocol()) {
            report(
                &name,
                "role differs from the installed declaration identity or visibility",
                attribute.span,
            );
            continue;
        }
        match items.entry(role) {
            Entry::Vacant(entry) => {
                entry.insert(id.clone());
            }
            Entry::Occupied(_) => report(&name, "duplicate role", attribute.span),
        }
    }
    if lowered.language_foundation {
        for role in LangRole::ALL {
            if !items.contains_key(&role) {
                report(role.name(), "missing required role", Span::default());
            }
        }
    }
    items
}

/// Compare analyzed member types/binders/parents against the installed checked
/// language product before any role-dependent body is accepted.
pub(crate) fn validate_shapes(
    declarations: &Declarations,
    aggregates: &AggregateCatalog,
    diagnostics: &mut DiagnosticBuffer,
) {
    let product = catalog::shared();
    for (role, id) in &declarations.language_items {
        let expected = product
            .traits
            .iter()
            .find(|item| item.name == role.protocol().name())
            .expect("required checked language product");
        let owner_generic_count = expected.generic_params.len();
        let valid = aggregates.trait_(id).is_some_and(|actual| {
            actual.storage_access == expected.storage_access
                && actual.conversion_adapter == expected.conversion_adapter
                && actual.generic_params.len() == expected.generic_params.len()
                && actual.bounds.values().all(Vec::is_empty)
                && actual.associated_consts.is_empty()
                && actual.associated_type_parameters.is_empty()
                && actual
                    .supertraits
                    .iter()
                    .map(lower_nominal_type)
                    .collect::<Vec<_>>()
                    == expected.supertraits
                && actual.associated_types.len() == expected.associated_types.len()
                && expected.associated_types.iter().all(|member| {
                    actual
                        .associated_types
                        .get(&member.declaration)
                        .is_some_and(|bounds| {
                            bounds
                                .iter()
                                .map(|bound| match bound {
                                    ConstraintTarget::Standard(bound) => {
                                        Constraint::Standard(*bound)
                                    }
                                    ConstraintTarget::Trait(bound) => {
                                        Constraint::Trait(lower_nominal_type(bound))
                                    }
                                })
                                .collect::<Vec<_>>()
                                == member.bounds
                        })
                })
                && actual.methods.len() == expected.methods.len()
                && actual
                    .methods
                    .iter()
                    .zip(&expected.methods)
                    .all(|(actual, expected)| {
                        actual.name == expected.name
                            && actual.default.is_none()
                            && actual.policy == expected.method_policy
                            && actual.generic_params.len()
                                == expected.generic_params.len() + owner_generic_count
                            && actual.bounds.values().all(Vec::is_empty)
                            && actual.params.len() == expected.params.len()
                            && actual.params.iter().zip(&expected.params).all(
                                |(actual, expected)| {
                                    !actual.ty.is_unresolved()
                                        && lower_type(&actual.ty) == expected.ty
                                },
                            )
                            && !actual.return_type.is_unresolved()
                            && lower_type(&actual.return_type) == expected.return_type
                    })
        });
        if !valid {
            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidLanguageRole {
                role: role.name().into(),
                reason: "declaration binders, parents or member shapes differ from the installed language contract".into(),
            }));
        }
    }
}
