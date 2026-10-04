use super::*;
use crate::{
    declare_analysis,
    host::HostDeclarations,
    imports::{ModuleGraph, types::TypeCatalog},
    lower,
    native::{api, render::declaration_source},
};
use kagari_common::{cancellation::CancellationToken, identity::map::DefinitionContext};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::{catalog as foundation_catalog, catalog};
use kagari_syntax::parser::parse_declarations;
use std::sync::Arc;

fn check_core(mutate_source: impl Fn(&mut String)) -> DiagnosticBuffer {
    let cancel = CancellationToken::default();
    let modules = catalog::shared();
    let lowered = modules
        .iter()
        .map(|module| {
            let mut generated = declaration_source(module, &foundation_catalog::shared()).unwrap();
            let (_, original) =
                api::import_source(module, &modules, &generated, Default::default(), &cancel)
                    .unwrap();
            mutate_source(&mut generated.text);
            if generated.text == original.source.text() {
                return original;
            }
            // Exercise the role/shape checker independently of the production
            // registration correspondence gate, which rejects these views earlier.
            let mut sources = SourceDatabase::default();
            sources
                .bind_module(&generated.uri, module.identity.clone())
                .unwrap();
            let file = sources
                .set(&generated.uri, generated.text, SourceLayer::Base)
                .unwrap();
            let source = sources.snapshot().file(file).unwrap().clone();
            let parsed = parse_declarations(&source, Default::default(), &cancel).unwrap();
            assert!(parsed.diagnostics().is_empty());
            let mut lowered = lower::lower_module_controlled(source, &parsed.syntax(), &cancel);
            lowered.language_foundation = original.language_foundation;
            lowered.registered_native_api = original.registered_native_api;
            lowered.native_package_alias = original.native_package_alias.clone();
            lowered.native_prelude = original.native_prelude;
            lowered.registered_traits = original.registered_traits.clone();
            lowered.native_array_interfaces = original.native_array_interfaces.clone();
            lowered
                .module
                .imports
                .extend(original.module.imports.clone());
            Arc::new(lowered)
        })
        .collect::<Vec<_>>();
    let hosts = HostDeclarations::empty();
    let graph = ModuleGraph::build(lowered.iter().map(Arc::as_ref), &hosts, &cancel).unwrap();
    let context = DefinitionContext::new().unwrap();
    let declared = lowered
        .into_iter()
        .map(|lowered| {
            let imports = graph
                .node(lowered.source.module_identity())
                .unwrap()
                .imports
                .clone();
            declare_analysis(lowered, hosts.clone(), imports, &context, &cancel)
        })
        .collect::<Vec<_>>();
    let types = TypeCatalog::new(declared.iter());
    let prepared = declared
        .iter()
        .map(|declared| {
            declared.clone().check_signatures(
                types
                    .bindings(&declared.names.facts.imports, &cancel)
                    .unwrap(),
                None,
                &cancel,
            )
        })
        .collect::<Vec<_>>();
    let mut aggregates = AggregateCatalog::default();
    for module in &prepared {
        aggregates
            .add_module(
                &module.lowered,
                &module.declarations,
                module.signatures.facts(),
                &cancel,
            )
            .unwrap();
    }
    let mut diagnostics = DiagnosticBuffer::default();
    for module in &prepared {
        diagnostics.extend(module.names.diagnostics.iter().cloned());
        diagnostics.extend(module.signatures.diagnostics().iter().cloned());
        validate_shapes(
            &module.declarations,
            &aggregates,
            &module.lowered.registered_traits,
            &mut diagnostics,
        );
    }
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
        .bind_module(
            uri,
            language::identity(kagari_types::language::Protocol::Add).module,
        )
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
    let result = crate::analyze_source(&source, foundation_catalog::shared())
        .expect("installed declaration analysis");
    assert!(
        result.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::InvalidLanguageRole { .. }
        ))
    );
}
