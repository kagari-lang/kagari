//! Header-time role collection. Roles identify ordinary traits, not a second trait model.
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
    types::semantic::{lower_nominal_type, lower_type},
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
    callable::CallableImplementation,
    declaration::{
        AssociatedTypeDef, FnDecl, Param, TraitDef, conversion::ConversionAdapter,
        names::NameNamespace,
    },
    language::{self, role::LangRole},
    ty::{GenericBound, GenericParam},
    visibility::Visibility,
};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    iter,
};

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
    declarations: &Declarations,
    aggregates: &AggregateCatalog,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
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
    for actual in aggregates.traits().filter(|actual| {
        actual.conversion_adapter.is_some()
            && matches!(
                declarations.definition_target(&actual.id),
                Some(ResolvedName::Trait(_))
            )
    }) {
        if cancel.check().is_err() {
            return;
        }
        let exposed = match conversion_exposed(actual, declarations, cancel) {
            Ok(exposed) => exposed,
            Err(_) => return,
        };
        if !exposed
            || !conversion_valid(actual, declarations.definitions())
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
    let targets = match &actual.conversion_adapter {
        Some(ConversionAdapter::Reverse { origin, result, .. }) => iter::once(origin)
            .chain(result.iter())
            .copied()
            .collect::<Vec<_>>(),
        Some(ConversionAdapter::Forward { result, .. }) => vec![*result],
        None => return Ok(true),
    };
    for target in targets {
        if declarations
            .resolve_definition_exposure(target, NameNamespace::Type, None, cancel)?
            .is_none()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn conversion_valid(actual: &TraitSignature, definitions: &DefinitionTable) -> bool {
    // Intrinsic derivation currently proves only the forward conversion. It cannot
    // discharge additional obligations on the reverse receiver or its method.
    if !actual.associated_consts.is_empty()
        || !actual.associated_type_parameters.is_empty()
        || !actual.supertraits.is_empty()
        || actual.bounds.values().any(|bounds| !bounds.is_empty())
        || actual
            .methods
            .iter()
            .any(|method| method.bounds.values().any(|bounds| !bounds.is_empty()))
    {
        return false;
    }
    // A temporary projection of the actual checked declaration lets the portable
    // verifier own conversion rules too; no expected trait catalog is retained.
    let record = TraitDef {
        associated_consts: vec![],
        name: String::new(),
        supertraits: actual.supertraits.iter().map(lower_nominal_type).collect(),
        bounds: actual
            .bounds
            .iter()
            .map(|(ty, constraints)| GenericBound {
                ty: lower_type(ty),
                constraints: constraints.iter().map(lower_nominal_type).collect(),
            })
            .collect(),
        generic_params: actual
            .generic_params
            .iter()
            .map(|parameter| GenericParam {
                owner: parameter.owner,
                position: parameter.position,
            })
            .collect(),
        methods: actual
            .methods
            .iter()
            .map(|method| FnDecl {
                host_policy: None,
                method_policy: method.policy,
                name: method.name.clone(),
                implementation: CallableImplementation::Required,
                generic_params: method
                    .generic_params
                    .iter()
                    .filter(|parameter| parameter.owner == method.id)
                    .map(|parameter| GenericParam {
                        owner: parameter.owner,
                        position: parameter.position,
                    })
                    .collect(),
                bounds: method
                    .bounds
                    .iter()
                    .map(|(ty, constraints)| GenericBound {
                        ty: lower_type(ty),
                        constraints: constraints.iter().map(lower_nominal_type).collect(),
                    })
                    .collect(),
                params: method
                    .params
                    .iter()
                    .map(|parameter| Param {
                        name: parameter.name.clone(),
                        ty: lower_type(&parameter.ty),
                        mutable: parameter.writeability.is_var(),
                    })
                    .collect(),
                return_type: lower_type(&method.return_type),
            })
            .collect(),
        associated_types: actual
            .associated_types
            .iter()
            .map(|(id, bounds)| AssociatedTypeDef {
                declaration: *id,
                generic_params: vec![],
                parameter_bounds: vec![],
                bounds: bounds.iter().map(lower_nominal_type).collect(),
            })
            .collect(),
        conversion_adapter: actual.conversion_adapter.clone(),
    };
    actual
        .conversion_adapter
        .as_ref()
        .is_some_and(|relationship| relationship.valid_in(&actual.id, &record, definitions))
}
