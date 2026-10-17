use super::*;
use crate::policy::Level;
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_hir::analysis::{AnalysisDatabase, AnalysisSnapshot};
use kagari_source::{
    diagnostic::Severity,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog;
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

fn findings(snapshot: &AnalysisSnapshot, root: FileId, policy: &Policy) -> Vec<DiagnosticRecord> {
    lint(snapshot, root, policy, &Default::default()).unwrap()
}

fn codes(records: &[DiagnosticRecord], code: &str) -> usize {
    records.iter().filter(|record| record.code == code).count()
}

#[test]
fn usage_tracks_reads_shadowing_captures_and_places() {
    let (_, _, snapshot, root) = analyze(
        &[(
            "root",
            r#"
        fn compute(parameter:i32, _intent:i32)->i32 {
            val shadow=1; val shadow=2;
            var rebound=0; rebound=1; rebound+=1;
            var object=[1]; object[0]=2;
            var captured=3; val closure=|argument:i32| captured+argument;
            val deferred=4;
            var stable=5;
            var write_only=0; val writer=|| {write_only=1;}; writer();
            shadow+rebound+object[0]+closure(deferred)+stable
        }
    "#,
        )],
        100,
    );
    snapshot.check_program(root, &Default::default()).unwrap();
    let records = findings(&snapshot, root, &Policy::default());
    assert_eq!(codes(&records, "unused_binding"), 2, "{records:?}");
    assert_eq!(codes(&records, "unused_parameter"), 1, "{records:?}");
    assert_eq!(codes(&records, "unnecessary_var"), 2, "{records:?}");

    let (_, _, snapshot, root) = analyze(
        &[(
            "root",
            "fn read()->i32 { var (changed, stable)=(1,2); val read=||changed; changed=3; val (_, unused)=(4,5); read()+stable }",
        )],
        100,
    );
    snapshot.check_program(root, &Default::default()).unwrap();
    let records = findings(&snapshot, root, &Policy::default());
    assert_eq!(codes(&records, "unused_binding"), 1, "{records:?}");
    assert_eq!(codes(&records, "unnecessary_var"), 1, "{records:?}");
}

#[test]
fn naming_rules_cover_declarations_and_preserve_trait_contracts() {
    let (_, _, snapshot, root) = analyze(
        &[(
            "root",
            r#"
        struct bad_type { val BadField:i32 }
        enum bad_enum { bad_variant }
        trait bad_trait { fn BadMethod(self, parameter:i32)->i32; }
        impl bad_trait for bad_type { fn BadMethod(self, parameter:i32)->i32 { parameter } }
        const bad_global:i32=1;
        fn BadFunction<bad_generic>(BadParameter:bad_generic) { val BadLocal=BadParameter; }
    "#,
        )],
        100,
    );
    snapshot.check_program(root, &Default::default()).unwrap();
    let records = findings(&snapshot, root, &Policy::default());
    assert_eq!(codes(&records, "non_upper_camel_case"), 5, "{records:?}");
    assert_eq!(codes(&records, "non_upper_case_globals"), 1, "{records:?}");
    assert_eq!(codes(&records, "non_snake_case"), 5, "{records:?}");
    assert_eq!(codes(&records, "unused_parameter"), 0, "{records:?}");
}

#[test]
fn partial_facts_and_truncation_do_not_authorize_semantic_rules() {
    for limit in [0, 1, 100] {
        for (broken, expected) in [
            ("fn broken()->i32{true}", 1),
            ("fn broken(value:Missing){}", 0),
        ] {
            let text =
                format!("{broken} fn healthy()->i32{{val unused=1;42}} struct bad_type {{}}");
            let (_, _, snapshot, root) = analyze(&[("root", &text)], limit);
            assert!(snapshot.check_program(root, &Default::default()).is_err());
            let records = findings(&snapshot, root, &Policy::default());
            assert_eq!(codes(&records, "unused_binding"), expected, "{records:?}");
            assert_eq!(
                codes(&records, "non_upper_camel_case"),
                1,
                "syntax rules remain available"
            );
        }
    }
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
            &findings(&snapshot, root, &Policy::default()),
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
    let records = findings(&snapshot, root, &Policy::default());
    assert_eq!(codes(&records, "unused_parameter"), 2, "{records:?}");
    assert_eq!(codes(&records, "KG_LINT_POLICY"), 1, "{records:?}");
    let mut policy = Policy {
        deny_warnings: true,
        ..Default::default()
    };
    policy
        .overrides
        .insert(LintId::UnusedParameter, Level::Allow);
    let records = findings(&snapshot, root, &policy);
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
    let warn = findings(&snapshot, root, &policy);
    assert_eq!(warn.len(), 1);
    assert_eq!(warn[0].severity, Severity::Warning);
    policy.deny_warnings = true;
    assert_eq!(
        findings(&snapshot, root, &policy)[0].severity,
        Severity::Error
    );
    policy.overrides.insert(LintId::UnusedBinding, Level::Allow);
    assert!(findings(&snapshot, root, &policy).is_empty());
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
fn valid_names_malformed_policy_and_pattern_parameters_have_distinct_contracts() {
    let (_, _, snapshot, root) = analyze(
        &[(
            "root",
            r#"
        struct HTTPServer { val _field:i32 }
        trait Contract { type Item; const LIMIT:i32; fn method(self, BadParameter:i32); }
        const _MAX_COUNT:i32=1;
        fn choose<T>(value:T)->i32 {
            val _intent=value;
            val closure=|unused:i32| 1;
            match (1,2) { (first, second) => first+closure(1) }
        }
        #[tool::allow()] fn empty(){}
        #[tool::allow(rule="unused_binding")] fn named(){}
        #[tool::allow(1)] fn number(){}
        #[tool::allow="unused_binding"] fn assigned(){}
    "#,
        )],
        100,
    );
    snapshot.check_program(root, &Default::default()).unwrap();
    let records = findings(&snapshot, root, &Policy::default());
    assert_eq!(codes(&records, "KG_LINT_POLICY"), 4, "{records:?}");
    assert_eq!(codes(&records, "non_upper_camel_case"), 0, "{records:?}");
    assert_eq!(codes(&records, "non_upper_case_globals"), 0, "{records:?}");
    assert_eq!(codes(&records, "non_snake_case"), 1, "{records:?}");
    assert_eq!(codes(&records, "unused_parameter"), 1, "{records:?}");
    assert_eq!(codes(&records, "unused_binding"), 1, "{records:?}");
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(lint(&snapshot, root, &Policy::default(), &cancel).is_err());
}
