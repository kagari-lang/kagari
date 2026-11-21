use super::*;
use crate::{
    analysis::{
        AnalysisDatabase, core_source::packaged_modules, external::foundation::change_export,
    },
    declare_analysis, lower,
    tests::standard::standard_modules,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use std::sync::Arc;

fn check_core(mutate_source: impl Fn(&mut String)) -> DiagnosticBuffer {
    let mut db = AnalysisDatabase::default();

    for input in packaged_modules() {
        let mut text = input.source.text().to_owned();
        mutate_source(&mut text);
        if text != input.source.text() {
            let mut altered = input.clone();
            change_export(&mut altered, |_| text);
            db.replace_core_fixture(altered);
        }
    }
    let snapshot = db
        .snapshot(Default::default(), &Default::default())
        .unwrap();
    snapshot
        .files()
        .flat_map(|file| file.result().diagnostics().iter().cloned())
        .collect()
}

#[test]
fn every_core_role_is_collected_from_source_and_has_a_checked_shape() {
    assert_eq!(LangRole::ALL.len(), 26);
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
        (
            "#[lang = \"eq\"]",
            "#[lang = \"eq\"] pub const MISPLACED: i32 = 0;",
            "role requires a trait declaration",
        ),
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
        ("pub trait Add<Rhs>", "pub trait Add<Rhs, Extra>", "add"),
        ("pub trait Eq: PartialEq", "pub trait Eq", "eq"),
        (
            "fn next(self) -> Option<Self::Item>;",
            "fn before(self) {} fn next(self) -> Option<Self::Item>;",
            "iterator",
        ),
        (
            "type Iter: Iterator<Item = Self::Item>;",
            "type Iter: Iterator;",
            "iterable",
        ),
        (
            "pub trait PartialEq {",
            "pub trait PartialEq { type Extra<T>;",
            "partial_eq",
        ),
        ("Some(T),", "Some(T, T),", "Option"),
        ("pub enum Ordering", "enum Ordering", "Ordering"),
        (
            "fn hash(self) -> i64;",
            "fn hash(self, extra: i32) -> i64;",
            "hash",
        ),
        ("type Output;", "type Output; type Extra;", "add"),
        ("pub trait Try:", "trait Try:", "try"),
        ("type Residual;", "type Residual; type Extra;", "try"),
        (
            "pub trait FromResidual<R>",
            "pub trait FromResidual<R, Extra>",
            "from_residual",
        ),
    ] {
        let diagnostics = check_core(|source| *source = source.replace(from, to));
        assert!(diagnostics.iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidLanguageRole { role: actual, .. } if actual == role)), "{diagnostics:?}");
    }
}

#[test]
fn canonical_module_identity_still_requires_the_complete_role_set() {
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
        Arc::default(),
        Arc::default(),
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    assert!(declared.record.names.diagnostics.iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidLanguageRole { reason, .. } if reason.contains("missing required role"))));
}

#[test]
fn application_traits_cannot_claim_roles() {
    let source = SourceFile::new(
        "memory://application-role.kgr",
        "#[lang = \"add\"] trait Add<Rhs> { type Output; fn add(self, rhs: Rhs) -> Self::Output; }",
    );
    let result =
        crate::analyze_source(&source, standard_modules()).expect("installed declaration analysis");
    assert!(
        result.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::InvalidLanguageRole { .. }
        ))
    );
}
