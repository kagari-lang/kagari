use std::slice;

use kagari_common::span::Span;
use kagari_source::{
    diagnostic::Severity,
    identity::{FileId, FileSpan, Revision},
    presentation::{DiagnosticLabel, DiagnosticRecord},
    source::SourceFile,
};
use serde_json::{Value, json};

use crate::tests::{
    common,
    diagnostics::{project_records, read_expectations},
};

const EXPECTED: &str = r#"[
  {"code":"ONE","severity":"error","primary":{"path":"test.kg","start":0,"end":2},
   "message":"first","labels":[
     {"span":{"path":"test.kg","start":0,"end":2},"message":"left"},
     {"span":{"path":"other.kg","start":0,"end":1},"message":"right"}],
   "notes":["first note","second note"]},
  {"code":"TWO","severity":"warning","primary":null,"message":"second","labels":[],"notes":[]}
]"#;

#[test]
fn schema_requires_exact_fields_and_types_without_defaults() {
    let source = common::source("π\r\nx");
    let other = SourceFile::new("other.kg", "y");
    let sources = [&source, &other];
    let valid: Value = serde_json::from_str(EXPECTED).unwrap();
    assert!(read_expectations(EXPECTED, &sources).is_ok());
    assert!(read_expectations("[]", &sources).unwrap().is_empty());
    for field in ["code", "severity", "primary", "message", "labels", "notes"] {
        let mut missing = valid.clone();
        missing[0].as_object_mut().unwrap().remove(field);
        assert!(
            read_expectations(&missing.to_string(), &sources).is_err(),
            "missing {field}"
        );
        for wrong in [Value::Null, json!(42), json!({})] {
            if field == "primary" && wrong.is_null() {
                continue;
            }
            let mut invalid = valid.clone();
            invalid[0][field] = wrong;
            assert!(
                read_expectations(&invalid.to_string(), &sources).is_err(),
                "wrong {field}"
            );
        }
    }
    for path in ["primary", "label", "label_span"] {
        let mut incomplete = valid.clone();
        let (object, field) = match path {
            "primary" => (&mut incomplete[0]["primary"], "path"),
            "label" => (&mut incomplete[0]["labels"][0], "message"),
            _ => (&mut incomplete[0]["labels"][0]["span"], "end"),
        };
        object.as_object_mut().unwrap().remove(field);
        assert!(
            read_expectations(&incomplete.to_string(), &sources).is_err(),
            "{path}"
        );
    }
    for text in [
        "[",
        "{}",
        "null",
        "[null]",
        "[] trailing",
        r#"[["ONE","error",null,"first",[],[]]]"#,
    ] {
        assert!(read_expectations(text, &sources).is_err(), "{text}");
    }
    // Parse raw JSON, not Value: Value would silently discard duplicate keys.
    for (field, replacement) in [
        ("\"code\":\"ONE\"", "\"code\":\"ONE\",\"code\":\"ONE\""),
        ("\"code\":\"ONE\"", "\"extra\":0,\"code\":\"ONE\""),
        ("\"start\":0", "\"start\":0,\"start\":0"),
        ("\"path\":\"test.kg\"", "\"extra\":0,\"path\":\"test.kg\""),
        (
            "\"message\":\"left\"",
            "\"message\":\"left\",\"message\":\"left\"",
        ),
        ("\"message\":\"left\"", "\"extra\":0,\"message\":\"left\""),
    ] {
        let invalid = EXPECTED.replace(field, replacement);
        assert_ne!(invalid, EXPECTED, "probe must change its input");
        assert!(
            read_expectations(&invalid, &sources).is_err(),
            "{replacement}"
        );
    }
}

#[test]
fn schema_validates_all_locations_and_ordered_element_types() {
    let source = common::source("π\r\nx");
    let other = SourceFile::new("other.kg", "y");
    let sources = [&source, &other];
    let valid: Value = serde_json::from_str(EXPECTED).unwrap();
    for span in [
        json!(null),
        json!({"path":"test.kg","start":2,"end":0}),
        json!({"path":"test.kg","start":0,"end":6}),
        json!({"path":"test.kg","start":1,"end":2}),
        json!({"path":"test.kg","start":0,"end":1}),
        json!({"path":"wrong.kg","start":0,"end":2}),
        json!({"path":"test.kg","start":-1,"end":2}),
        json!({"path":"test.kg","start":0.0,"end":2}),
        json!({"path":"test.kg","start":"0","end":2}),
        json!({"path":null,"start":0,"end":2}),
        json!(["test.kg", 0, 2]),
    ] {
        let mut invalid = valid.clone();
        invalid[0]["labels"][0]["span"] = span.clone();
        assert!(
            read_expectations(&invalid.to_string(), &sources).is_err(),
            "label {span}"
        );
        if !span.is_null() {
            invalid = valid.clone();
            invalid[0]["primary"] = span;
            assert!(
                read_expectations(&invalid.to_string(), &sources).is_err(),
                "primary"
            );
        }
    }
    for (field, value) in [
        ("severity", json!("info")),
        ("severity", json!({"error": null})),
        (
            "labels",
            json!([[{"path":"test.kg","start":0,"end":2}, "left"]]),
        ),
        ("labels", json!([null])),
        ("notes", json!([null])),
        ("notes", json!([42])),
    ] {
        let mut invalid = valid.clone();
        invalid[0][field] = value;
        assert!(
            read_expectations(&invalid.to_string(), &sources).is_err(),
            "{field}"
        );
    }
}

#[test]
fn projection_rejects_uncaptured_stale_and_invalid_primary_and_label_spans() {
    let source = common::source("π\r\nx");
    let span = source.span(Span::new(0, 2)).unwrap();
    let record = DiagnosticRecord {
        code: "ONE".into(),
        severity: Severity::Error,
        span: Some(span),
        message: "first".into(),
        labels: vec![],
        notes: vec![],
    };
    let mut wrong_file = span;
    wrong_file.file = FileId::fresh();
    let mut stale = span;
    stale.revision = Revision(1);
    let invalid = [
        wrong_file,
        stale,
        FileSpan {
            range: Span::new(1, 2),
            ..span
        },
        FileSpan {
            range: Span::new(0, 6),
            ..span
        },
        FileSpan {
            range: Span::new(2, 0),
            ..span
        },
    ];
    for span in invalid {
        let mut primary = record.clone();
        primary.span = Some(span);
        assert!(project_records(&[primary], &[&source]).is_err());
        let mut label = record.clone();
        label.labels.push(DiagnosticLabel {
            span,
            message: "bad".into(),
        });
        assert!(project_records(&[label], &[&source]).is_err());
    }
    let same_name_wrong_file = common::source(source.text());
    assert!(project_records(slice::from_ref(&record), &[&same_name_wrong_file]).is_err());
    assert!(project_records(&[record], &[&source, &same_name_wrong_file]).is_err());
    assert!(read_expectations(EXPECTED, &[&source, &same_name_wrong_file]).is_err());
}

#[test]
fn comparison_detects_every_field_count_and_order_without_normalization() {
    let source = common::source("π\r\nx");
    let other = SourceFile::new("other.kg", "y");
    let sources = [&source, &other];
    let left = source.span(Span::new(0, 2)).unwrap();
    let right = other.span(Span::new(0, 1)).unwrap();
    let records = vec![
        DiagnosticRecord {
            code: "ONE".into(),
            severity: Severity::Error,
            span: Some(left),
            message: "first".into(),
            labels: vec![
                DiagnosticLabel {
                    span: left,
                    message: "left".into(),
                },
                DiagnosticLabel {
                    span: right,
                    message: "right".into(),
                },
            ],
            notes: vec!["first note".into(), "second note".into()],
        },
        DiagnosticRecord {
            code: "TWO".into(),
            severity: Severity::Warning,
            span: None,
            message: "second".into(),
            labels: vec![],
            notes: vec![],
        },
    ];
    let expected = read_expectations(EXPECTED, &sources).unwrap();
    assert_eq!(project_records(&records, &sources).unwrap(), expected);
    let changes: &[fn(&mut Vec<DiagnosticRecord>)] = &[
        |r| r[0].code.push('!'),
        |r| r[0].severity = Severity::Warning,
        |r| r[0].message.push(' '),
        |r| r[0].span = None,
        |r| r[0].span.as_mut().unwrap().range.end = 4,
        |r| r[0].span = Some(r[0].labels[1].span),
        |r| r[0].labels[0].span.range.end = 4,
        |r| r[0].labels[0].span = r[0].labels[1].span,
        |r| r[0].labels[0].message.push('!'),
        |r| r[0].labels.swap(0, 1),
        |r| {
            r[0].labels.pop();
        },
        |r| {
            let label = r[0].labels[0].clone();
            r[0].labels.push(label);
        },
        |r| r[0].notes.swap(0, 1),
        |r| r[0].notes[0].push('!'),
        |r| {
            r[0].notes.pop();
        },
        |r| r[0].notes.push("extra".into()),
        |r| r.swap(0, 1),
        |r| {
            r.pop();
        },
        |r| r.push(r[0].clone()),
    ];
    for (index, change) in changes.iter().enumerate() {
        let mut changed = records.clone();
        change(&mut changed);
        assert_ne!(
            project_records(&changed, &sources).unwrap(),
            expected,
            "mutation {index}"
        );
    }
}
