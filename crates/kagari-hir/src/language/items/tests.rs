use super::*;
use crate::{
    declare_analysis,
    host::HostDeclarations,
    imports::{ModuleGraph, types::TypeCatalog},
    lower,
    native::{api, render::declaration_source},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::map::DefinitionContext,
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use std::sync::Arc;

fn check_core(mutate: impl FnOnce(&mut String)) -> DiagnosticBuffer {
    let cancel = CancellationToken::default();
    let module = catalog::shared();
    let mut generated = declaration_source(&module).unwrap();
    mutate(&mut generated.text);
    let (_, lowered) = api::import_source(
        &module,
        &[module.clone()],
        &generated,
        Default::default(),
        &cancel,
    )
    .unwrap();
    let hosts = HostDeclarations::empty();
    let graph = ModuleGraph::build([lowered.as_ref()], &hosts, &cancel).unwrap();
    let imports = graph
        .node(&language::module_identity())
        .unwrap()
        .imports
        .clone();
    let declared = declare_analysis(
        lowered,
        hosts,
        imports,
        &DefinitionContext::new().unwrap(),
        &cancel,
    );
    let types = TypeCatalog::new([&declared])
        .bindings(&declared.names.facts.imports, &cancel)
        .unwrap();
    let prepared = declared.check_signatures(types, None, &cancel);
    let mut aggregates = AggregateCatalog::default();
    aggregates
        .add_module(
            &prepared.lowered,
            &prepared.declarations,
            prepared.signatures.facts(),
            &cancel,
        )
        .unwrap();
    let mut diagnostics = prepared.names.diagnostics.clone();
    diagnostics.extend(prepared.signatures.diagnostics().iter().cloned());
    validate_shapes(&prepared.declarations, &aggregates, &mut diagnostics);
    diagnostics
}

#[test]
fn every_core_role_is_collected_from_source_and_has_a_checked_shape() {
    assert_eq!(LangRole::ALL.len(), 24);
    assert!(check_core(|_| {}).is_empty());
}

#[test]
fn missing_duplicate_unknown_and_malformed_roles_are_rejected() {
    for (from, to, reason) in [
        (
            "#[lang = \"eq\"]",
            "#[meta = \"eq\"]",
            "missing required role",
        ),
        (
            "#[lang = \"eq\"]",
            "#[lang = \"??\"]",
            "unknown reserved role",
        ),
        ("#[lang = \"eq\"]", "#[lang(\"eq\")] ", "expected #[lang"),
    ] {
        let diagnostics = check_core(|source| *source = source.replace(from, to));
        assert!(diagnostics.iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidLanguageRole { reason: actual, .. } if actual.contains(reason))), "{diagnostics:?}");
    }
    let diagnostics = check_core(|source| {
        *source = source.replace("#[lang = \"eq\"]", "#[lang = \"eq\"]\n#[lang = \"eq\"]")
    });
    assert!(diagnostics.iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidLanguageRole { reason, .. } if reason == "duplicate role")), "{diagnostics:?}");
}

#[test]
fn incorrect_language_member_types_are_rejected() {
    let diagnostics = check_core(|source| {
        *source = source.replace("fn hash(self) -> i64;", "fn hash(self) -> i32;")
    });
    assert!(diagnostics.iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidLanguageRole { role, reason } if role == "hash" && reason.contains("member shapes"))), "{diagnostics:?}");
}

#[test]
fn incorrect_role_visibility_binders_parents_and_members_are_rejected() {
    for (from, to, role) in [
        ("pub trait Hash", "trait Hash", "hash"),
        ("pub trait Add<T0>", "pub trait Add<T0, Extra>", "add"),
        ("pub trait Eq: PartialEq", "pub trait Eq", "eq"),
        (
            "fn hash(self) -> i64;",
            "fn hash(self, extra: i32) -> i64;",
            "hash",
        ),
        ("type Output;", "type Output; type Extra;", "add"),
    ] {
        let diagnostics = check_core(|source| *source = source.replace(from, to));
        assert!(diagnostics.iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidLanguageRole { role: actual, .. } if actual == role)), "{diagnostics:?}");
    }
}

#[test]
fn copied_module_identity_does_not_authorize_application_roles() {
    let mut sources = SourceDatabase::default();
    let uri = "memory://counterfeit.kgr";
    sources
        .bind_module(uri, language::module_identity())
        .unwrap();
    let id = sources.set(uri, "#[lang = \"add\"] pub trait Add<Rhs> { type Output; fn add(self, rhs: Rhs) -> Self::Output; }".into(), SourceLayer::Base).unwrap();
    let source = sources.snapshot().file(id).unwrap().clone();
    let lowered = Arc::new(lower::lower_module(&source));
    let declared = declare_analysis(
        lowered,
        HostDeclarations::empty(),
        Arc::default(),
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    );
    assert!(declared.names.diagnostics.iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidLanguageRole { reason, .. } if reason.contains("only installed"))));
}

#[test]
fn application_traits_cannot_claim_roles() {
    let source = SourceFile::new(
        "memory://application-role.kgr",
        "#[lang = \"add\"] trait Add<Rhs> { type Output; fn add(self, rhs: Rhs) -> Self::Output; }",
    );
    let result = crate::analyze_source(&source);
    assert!(
        result.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::InvalidLanguageRole { .. }
        ))
    );
}
