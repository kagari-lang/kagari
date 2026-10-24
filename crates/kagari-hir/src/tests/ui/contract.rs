use std::{collections::BTreeSet, fs, path::PathBuf};

use crate::{
    analysis::AnalysisDatabase,
    tests::ui_diagnostics::{project_records, read_expectations},
};
use kagari_common::span::Span;
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};

use super::support::bind_records;

#[test]
fn fixture_inventory_has_exact_pairs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui");
    for (group, logical_name) in [
        ("bare", "test.kg"),
        ("standard", "test.kg"),
        ("local_functions", "locals.kgr"),
    ] {
        let mut inputs = BTreeSet::new();
        let mut expectations = BTreeSet::new();
        for entry in fs::read_dir(root.join(group)).expect("HIR UI group exists") {
            let entry = entry.expect("read HIR UI entry");
            assert!(
                entry.file_type().unwrap().is_file(),
                "unexpected UI directory"
            );
            let name = entry.file_name().into_string().expect("UTF-8 fixture name");
            if let Some(stem) = name.strip_suffix(".kgr").filter(|stem| !stem.is_empty()) {
                inputs.insert(stem.to_owned());
            } else if let Some(stem) = name
                .strip_suffix(".diagnostics.json")
                .filter(|stem| !stem.is_empty())
            {
                expectations.insert(stem.to_owned());
            } else {
                panic!("unexpected HIR UI fixture: {group}/{name}");
            }
        }
        assert!(!inputs.is_empty(), "empty HIR UI group: {group}");
        assert_eq!(inputs, expectations, "unpaired HIR UI fixtures: {group}");
        for stem in inputs {
            let path = root.join(group).join(format!("{stem}.kgr"));
            let source = SourceFile::new(logical_name, fs::read_to_string(&path).unwrap());
            let expected_path = path.with_extension("diagnostics.json");
            let expected = fs::read_to_string(&expected_path).unwrap();
            read_expectations(&expected, &[&source])
                .unwrap_or_else(|error| panic!("{}: {error}", expected_path.display()));
        }
    }
}

#[test]
fn raw_spans_require_captured_source_identity_revision_and_utf8_ranges() {
    let text = "fn bad() { missing; } // π\r\n";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("mem://diagnostic", text.into(), SourceLayer::Base)
        .unwrap();
    let captured = sources.snapshot();
    let analysis = AnalysisDatabase::default()
        .snapshot(captured.clone(), &Default::default())
        .unwrap();
    let file = analysis.file(id).unwrap();
    let diagnostics = file.result().diagnostics();
    assert_eq!(diagnostics.len(), 1);
    let records = bind_records(diagnostics, file.source(), &captured).unwrap();
    let span = records[0].span.expect("real diagnostic is located");
    assert_eq!(span.file, id);
    assert_eq!(span.revision, file.source().revision());
    assert!(captured.contains(span));
    project_records(&records, &[file.source()]).unwrap();

    let foreign = SourceFile::new(file.source().name(), text);
    assert!(
        bind_records(diagnostics, &foreign, &captured)
            .unwrap_err()
            .contains("identity")
    );
    sources
        .set(
            "mem://diagnostic",
            "fn good() {}".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    assert!(
        bind_records(diagnostics, file.source(), &sources.snapshot())
            .unwrap_err()
            .contains("revision")
    );

    let mut invalid = diagnostics[0].clone();
    let start = text.find('π').unwrap();
    invalid.span = Some(Span::new(start + 1, start + 2));
    assert!(
        bind_records(&[invalid], file.source(), &captured)
            .unwrap_err()
            .contains("raw HIR span")
    );
    assert_eq!(file.source().text(), text);
    assert_eq!(
        bind_records(diagnostics, file.source(), &captured).unwrap(),
        records
    );
}
