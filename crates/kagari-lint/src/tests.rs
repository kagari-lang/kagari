use super::*;
use crate::policy::Level;
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_hir::analysis::{AnalysisDatabase, AnalysisSnapshot};
use kagari_source::{
    diagnostic::Severity,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog;
use rstest::rstest;
use std::sync::Arc;

fn analyze(
    files: &[(&str, &str)],
    limit: usize,
) -> (AnalysisDatabase, SourceDatabase, AnalysisSnapshot, FileId) {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (path, text) in files {
        let parts = if *path == "root" {
            Vec::new()
        } else {
            path.split('/').map(str::to_owned).collect()
        };
        let name = format!("{path}.kgr");
        sources
            .bind_module(
                &name,
                ModuleIdentity {
                    package: PackageId("test".into()),
                    path: parts,
                },
            )
            .unwrap();
        let id = sources
            .set(&name, (*text).into(), SourceLayer::Base)
            .unwrap();
        root.get_or_insert(id);
    }
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared().unwrap());
    database.set_max_semantic_diagnostics(limit);
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    (database, sources, snapshot, root.unwrap())
}

fn codes(records: &[DiagnosticRecord], code: &str) -> usize {
    records.iter().filter(|record| record.code == code).count()
}

#[test]
fn static_partial_facts_exclude_broken_initializers() {
    let (_, _, snapshot, root) = analyze(
        &[(
            "root",
            "static val BROKEN: i32 = { val unused = 1; true }; static val HEALTHY: i32 = { val unused = 2; 3 };",
        )],
        100,
    );
    assert!(snapshot.check_program(root, &Default::default()).is_err());
    let records = lint(&snapshot, root, &Policy::default(), &Default::default()).unwrap();
    assert_eq!(codes(&records, "unused_binding"), 1, "{records:?}");
}

#[rstest]
#[case::body_error_limit_zero(0, "fn broken()->i32{true}", 1)]
#[case::body_error_limit_one(1, "fn broken()->i32{true}", 1)]
#[case::body_error_limit_hundred(100, "fn broken()->i32{true}", 1)]
#[case::shared_type_error_limit_zero(0, "fn broken(value:Missing){}", 0)]
#[case::shared_type_error_limit_one(1, "fn broken(value:Missing){}", 0)]
#[case::shared_type_error_limit_hundred(100, "fn broken(value:Missing){}", 0)]
fn partial_facts_and_truncation_do_not_authorize_semantic_rules(
    #[case] limit: usize,
    #[case] broken: &str,
    #[case] expected: usize,
) {
    let text = format!("{broken} fn healthy()->i32{{val unused=1;42}} struct bad_type {{}}");
    let (_, _, snapshot, root) = analyze(&[("root", &text)], limit);
    assert!(snapshot.check_program(root, &Default::default()).is_err());
    let records = lint(&snapshot, root, &Policy::default(), &Default::default()).unwrap();
    assert_eq!(codes(&records, "unused_binding"), expected, "{records:?}");
    assert_eq!(
        codes(&records, "non_upper_camel_case"),
        1,
        "syntax rules remain available"
    );
}

#[test]
fn invalid_child_facts_do_not_authorize_semantic_rules() {
    let (_, _, snapshot, root) = analyze(
        &[
            ("root", "mod child; fn healthy(){val unused=1;}"),
            ("child", "fn broken(value:Missing){}"),
        ],
        0,
    );
    assert!(snapshot.check_program(root, &Default::default()).is_err());
    assert_eq!(
        codes(
            &lint(&snapshot, root, &Policy::default(), &Default::default()).unwrap(),
            "unused_binding"
        ),
        0
    );
}

#[test]
fn policy_inherits_external_and_inline_modules_and_validates_even_when_allowed() {
    let (_, _, snapshot, root) = analyze(
        &[
            (
                "root",
                r#"#[tool::allow("unused_parameter")] mod child; #[tool::allow("unused_parameter")] mod inline { fn quiet(value:i32){} #[tool::warn("unused_parameter")] fn loud(value:i32){} }"#,
            ),
            (
                "child",
                r#"fn quiet(value:i32){} #[tool::warn("unused_parameter")] fn loud(value:i32){} #[tool::deny("unknown")] fn invalid(value:i32){}"#,
            ),
        ],
        100,
    );
    // Ordinary language checking treats tool metadata as opaque.
    snapshot.check_program(root, &Default::default()).unwrap();
    let records = lint(&snapshot, root, &Policy::default(), &Default::default()).unwrap();
    assert_eq!(codes(&records, "unused_parameter"), 2, "{records:?}");
    assert_eq!(codes(&records, "KG_LINT_POLICY"), 1, "{records:?}");
    let mut policy = Policy {
        deny_warnings: true,
        ..Default::default()
    };
    policy
        .overrides
        .insert(LintId::UnusedParameter, Level::Allow);
    let records = lint(&snapshot, root, &policy, &Default::default()).unwrap();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].severity, Severity::Error);
}

#[test]
fn levels_reuse_the_same_analysis_and_do_not_suppress_language_errors() {
    let (mut database, sources, snapshot, root) = analyze(
        &[("root", "fn broken()->i32{true} fn healthy(){val unused=1;}")],
        100,
    );
    let mut policy = Policy::default();
    let warn = lint(&snapshot, root, &policy, &Default::default()).unwrap();
    assert_eq!(warn.len(), 1);
    assert_eq!(warn[0].severity, Severity::Warning);
    policy.deny_warnings = true;
    assert_eq!(
        lint(&snapshot, root, &policy, &Default::default()).unwrap()[0].severity,
        Severity::Error
    );
    policy.overrides.insert(LintId::UnusedBinding, Level::Allow);
    assert!(
        lint(&snapshot, root, &policy, &Default::default())
            .unwrap()
            .is_empty()
    );
    assert!(snapshot.check_program(root, &Default::default()).is_err());
    let next = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(Arc::ptr_eq(
        snapshot.file(root).unwrap(),
        next.file(root).unwrap()
    ));
}

#[test]
fn pre_cancelled_lint_returns_no_findings() {
    let (_, _, snapshot, root) = analyze(
        &[(
            "root",
            include_str!("../tests/fixtures/rules/valid-names-and-malformed-policy.kgr"),
        )],
        100,
    );
    snapshot.check_program(root, &Default::default()).unwrap();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(lint(&snapshot, root, &Policy::default(), &cancel).is_err());
}

#[rstest]
#[case::reads_shadowing_captures_places(include_str!("../tests/fixtures/rules/reads-shadowing-captures-places.kgr"), &[("unused_binding", 2), ("unused_parameter", 1), ("unnecessary_var", 2)])]
#[case::patterns_and_struct_update_capture(include_str!("../tests/fixtures/rules/patterns-and-struct-update-capture.kgr"), &[("unused_binding", 1), ("unnecessary_var", 1)])]
#[case::async_block_capture(include_str!("../tests/fixtures/rules/async-block-capture.kgr"), &[("unused_binding", 1), ("unused_parameter", 0), ("unnecessary_var", 0)])]
#[case::declaration_naming_and_trait_contracts(include_str!("../tests/fixtures/rules/declaration-naming-and-trait-contracts.kgr"), &[("non_upper_camel_case", 7), ("KG_LINT_POLICY", 0), ("non_upper_case_globals", 1), ("non_snake_case", 5), ("unused_parameter", 0)])]
#[case::local_function_usage_and_lexical_policy(include_str!("../tests/fixtures/rules/local-function-usage-and-lexical-policy.kgr"), &[("unused_binding", 1), ("unused_parameter", 2), ("non_snake_case", 1)])]
#[case::eligible_static_initializers(include_str!("../tests/fixtures/rules/eligible-static-initializers.kgr"), &[("non_upper_case_globals", 1), ("non_snake_case", 1), ("unused_binding", 1), ("unnecessary_var", 1), ("KG_LINT_POLICY", 0)])]
#[case::valid_names_and_malformed_policy(include_str!("../tests/fixtures/rules/valid-names-and-malformed-policy.kgr"), &[("KG_LINT_POLICY", 4), ("non_upper_camel_case", 0), ("non_upper_case_globals", 0), ("non_snake_case", 1), ("unused_parameter", 1), ("unused_binding", 1)])]
fn ordinary_rules(#[case] source: &str, #[case] expected: &[(&str, usize)]) {
    let (_, _, snapshot, root) = analyze(&[("root", source)], 100);
    snapshot.check_program(root, &Default::default()).unwrap();
    let records = lint(&snapshot, root, &Policy::default(), &Default::default()).unwrap();
    for &(code, count) in expected {
        assert_eq!(codes(&records, code), count, "{records:?}");
    }
}
