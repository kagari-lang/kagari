//! Header-time role collection. Roles identify ordinary traits, not a second trait model.
#[cfg(test)]
mod tests;
use crate::{
    DiagnosticBuffer,
    aggregates::AggregateCatalog,
    aggregates::traits::MethodDefault,
    declarations::{DeclarationId, Declarations},
    hir::{
        attribute::{AttributeForm, AttributeTarget, AttributeValue},
        item::Item,
    },
    lower::LoweredModule,
    native::NativeBinding,
    resolver::resolved::ResolvedName,
    types::semantic::{lower_nominal_type, lower_type},
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId, span::Span};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    literal::decode_string_literal,
};
use kagari_types::{
    callable::CallableImplementation, declaration::TraitDef, language, language::role::LangRole,
    ty::GenericBound, visibility::Visibility,
};
use std::collections::{BTreeMap, btree_map::Entry};

/// Collects canonical `#[lang]` identities from an explicitly admitted foundation.
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
        if !lowered.language_foundation
            || lowered.source.module_identity() != &language::identity(role.protocol()).module
        {
            report(
                &name,
                "only admitted language foundation source may declare roles",
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
    if lowered.language_foundation {
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

/// Compare foundation and static conversion shapes before role-dependent bodies.
/// Offline defaults remain either complete checked Script bodies or unbound Native
/// declarations; neither supplies the installed recipe's execution authority.
pub(crate) fn validate_shapes(
    declarations: &Declarations,
    aggregates: &AggregateCatalog,
    expected: &BTreeMap<DefinitionId, TraitDef>,
    external: &BTreeMap<DefinitionId, TraitDef>,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let mut contracts = declarations
        .language_items
        .iter()
        .map(|(role, id)| {
            (
                role.name(),
                id,
                expected.get(id).or_else(|| external.get(id)),
                external.contains_key(id),
            )
        })
        .collect::<Vec<_>>();
    contracts.extend(
        external
            .iter()
            .filter(|(id, _)| !declarations.language_items.values().any(|role| role == *id))
            .map(|(id, contract)| (contract.name.as_str(), id, Some(contract), true)),
    );
    for (name, id, expected, offline) in contracts {
        let Some(expected) = expected else {
            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidLanguageRole {
                role: name.into(),
                reason: "missing static language declaration evidence".into(),
            }));
            continue;
        };
        let owner_generic_count = expected.generic_params.len();
        let valid = aggregates.trait_(id).is_some_and(|actual| {
            actual.conversion_adapter == expected.conversion_adapter
                && actual.generic_params.len() == expected.generic_params.len()
                && actual.bounds.values().all(Vec::is_empty)
                && expected.bounds.is_empty()
                && actual.associated_consts.is_empty()
                && expected.associated_consts.is_empty()
                && actual.associated_type_parameters.is_empty()
                && expected.associated_types.iter().all(|member| {
                    member.generic_params.is_empty() && member.parameter_bounds.is_empty()
                })
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
                            bounds.iter().map(lower_nominal_type).collect::<Vec<_>>()
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
                            && match &expected.implementation {
                                CallableImplementation::NativeDefault(_) if offline => {
                                    matches!(
                                        actual.default,
                                        Some(
                                            MethodDefault::NativeDeclaration
                                                | MethodDefault::Script
                                        )
                                    )
                                }
                                CallableImplementation::Native(_) if offline => {
                                    actual.default == Some(MethodDefault::NativeDeclaration)
                                }
                                CallableImplementation::Script if offline => {
                                    actual.default == Some(MethodDefault::Script)
                                }
                                CallableImplementation::NativeDefault(application) => {
                                    actual.default
                                        == Some(MethodDefault::Native(NativeBinding::Default(
                                            application.clone(),
                                        )))
                                }
                                CallableImplementation::Required => actual.default.is_none(),
                                _ => false,
                            }
                            && actual.policy == expected.method_policy
                            && actual.generic_params.len()
                                == expected.generic_params.len() + owner_generic_count
                            && {
                                let bounds = actual
                                    .bounds
                                    .iter()
                                    .filter(|(_, bounds)| !bounds.is_empty())
                                    .map(|(ty, bounds)| {
                                        let constraints =
                                            bounds.iter().map(lower_nominal_type).collect();
                                        GenericBound {
                                            ty: lower_type(ty),
                                            constraints,
                                        }
                                    })
                                    .collect::<Vec<_>>();
                                GenericBound::normalize(bounds, declarations.definitions(), cancel)
                                    .is_ok_and(|bounds| bounds == expected.bounds)
                            }
                            && actual.params.len() == expected.params.len()
                            && actual.params.iter().zip(&expected.params).all(
                                |(actual, expected)| {
                                    actual.name == expected.name
                                        && actual.writeability.is_var() == expected.mutable
                                        && !actual.ty.is_unresolved()
                                        && lower_type(&actual.ty) == expected.ty
                                },
                            )
                            && !actual.return_type.is_unresolved()
                            && lower_type(&actual.return_type) == expected.return_type
                    })
        });
        if !valid {
            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidLanguageRole {
                role: name.into(),
                reason: "declaration binders, parents or member shapes differ from the static language contract".into(),
            }));
        }
    }
}
