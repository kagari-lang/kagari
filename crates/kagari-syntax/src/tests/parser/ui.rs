use std::{collections::BTreeSet, fs, path::PathBuf};

use kagari_common::span::Span;
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    presentation::DiagnosticRecord,
    source::SourceFile,
};
use rstest::rstest;

use crate::{
    parser::parse,
    tests::{
        common,
        diagnostics::{project_records, read_expectations},
    },
};

#[rstest]
fn diagnostic_fixtures(#[files("tests/fixtures/ui/parser/*.kgr")] path: PathBuf) {
    let input = fs::read_to_string(&path).expect("UI source must be readable UTF-8");
    let source = common::source(&input);
    let parsed = parse(&source);
    let records = parser_records(parsed.diagnostics(), &source).expect("valid parser spans");
    let expected_path = path.with_extension("diagnostics.json");
    let text = fs::read_to_string(&expected_path)
        .unwrap_or_else(|error| panic!("{}: {error}", expected_path.display()));
    let expected = read_expectations(&text, &[&source])
        .unwrap_or_else(|error| panic!("{}: {error}", expected_path.display()));
    let received = project_records(&records, &[&source]).expect("captured parser source");
    assert_eq!(
        received,
        expected,
        "{} (logical source: test.kg)",
        path.display()
    );
}

fn parser_records(
    diagnostics: &[Diagnostic],
    source: &SourceFile,
) -> Result<Vec<DiagnosticRecord>, String> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            // The real adapter turns invalid ranges into None. Reject them first,
            // so an invalid located error cannot match an anonymous expectation.
            if let Some(span) = diagnostic.span
                && source.span(span).is_none()
            {
                return Err(format!("invalid raw parser span: {span:?}"));
            }
            Ok(DiagnosticRecord::from_diagnostic(
                diagnostic.clone(),
                source,
            ))
        })
        .collect()
}

fn validate_pairs(names: &[&str]) -> Result<(), String> {
    let mut inputs = BTreeSet::new();
    let mut expectations = BTreeSet::new();
    for name in names {
        if *name == ".gitattributes" {
            continue;
        }
        if let Some(stem) = name.strip_suffix(".kgr").filter(|stem| !stem.is_empty()) {
            inputs.insert(stem);
        } else if let Some(stem) = name
            .strip_suffix(".diagnostics.json")
            .filter(|stem| !stem.is_empty())
        {
            expectations.insert(stem);
        } else {
            return Err(format!("unexpected parser UI fixture extension: {name}"));
        }
    }
    if inputs.is_empty() {
        return Err("parser UI inputs must not be empty".into());
    }
    if inputs != expectations {
        return Err(format!(
            "unpaired parser UI fixtures: inputs={inputs:?}, expectations={expectations:?}"
        ));
    }
    Ok(())
}

#[test]
fn fixture_inventory_has_exact_pairs() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui/parser");
    let names = fs::read_dir(&directory)
        .expect("parser UI fixture directory exists")
        .map(|entry| {
            let entry = entry.expect("read parser UI entry");
            assert!(
                entry.file_type().unwrap().is_file(),
                "unexpected UI directory"
            );
            entry.file_name().into_string().expect("UTF-8 fixture name")
        })
        .collect::<Vec<_>>();
    validate_pairs(&names.iter().map(String::as_str).collect::<Vec<_>>()).unwrap();
}

#[rstest]
#[case::missing(&["case.kgr"], "unpaired")]
#[case::orphan(&["case.kgr", "case.diagnostics.json", "orphan.diagnostics.json"], "unpaired")]
#[case::wrong_extension(&["case.kgr", "case.diagnostic.json"], "extension")]
#[case::empty(&[], "empty")]
fn fixture_inventory_rejects_invalid_pairs(#[case] names: &[&str], #[case] error: &str) {
    assert!(validate_pairs(names).unwrap_err().contains(error));
}

#[test]
fn invalid_raw_parser_spans_cannot_become_anonymous() {
    let source = common::source("π");
    let diagnostic = Diagnostic::error(DiagnosticKind::ExpectedBlockEnd).with_span(Span::new(1, 2));
    assert!(
        DiagnosticRecord::from_diagnostic(diagnostic.clone(), &source)
            .span
            .is_none()
    );
    assert!(
        parser_records(&[diagnostic], &source)
            .unwrap_err()
            .contains("raw parser span")
    );
}
