use super::{fixture_root, fixtures, wire};
use crate::support::Project;
use rstest::rstest;
use serde_json::{Value, json};
use std::fs;

#[derive(Debug, Clone, Copy)]
enum Fault {
    MissingSource,
    MissingStdout,
    MissingExit,
    Orphan,
    UnknownField,
    DuplicateField,
    MissingField,
    MissingLabelLocation,
    WrongType,
    InvalidRange,
    Revision,
    MissingSummary,
    SummaryOrder,
    BlankLine,
    InvalidExit,
    InvalidUtf8,
    PositionalObject(&'static str, &'static [&'static str]),
    DuplicateType,
    DuplicateSummaryField,
    UnknownSummaryField,
}

#[rstest]
#[case::missing_source(Fault::MissingSource)]
#[case::missing_stdout(Fault::MissingStdout)]
#[case::missing_exit(Fault::MissingExit)]
#[case::orphan(Fault::Orphan)]
#[case::unknown_field(Fault::UnknownField)]
#[case::duplicate_field(Fault::DuplicateField)]
#[case::missing_nullable_field(Fault::MissingField)]
#[case::missing_label_location(Fault::MissingLabelLocation)]
#[case::wrong_field_type(Fault::WrongType)]
#[case::invalid_source_range(Fault::InvalidRange)]
#[case::wrong_revision(Fault::Revision)]
#[case::missing_summary(Fault::MissingSummary)]
#[case::summary_order(Fault::SummaryOrder)]
#[case::blank_line(Fault::BlankLine)]
#[case::noncanonical_exit(Fault::InvalidExit)]
#[case::invalid_utf8(Fault::InvalidUtf8)]
#[case::positional_position(Fault::PositionalObject("/location/start", &["line", "column"]))]
#[case::positional_location(Fault::PositionalObject(
    "/location",
    &["path", "revision", "byte_start", "byte_end", "start", "end"]
))]
#[case::positional_label(Fault::PositionalObject("/labels/0", &["location", "message"]))]
#[case::duplicate_type(Fault::DuplicateType)]
#[case::duplicate_summary_field(Fault::DuplicateSummaryField)]
#[case::unknown_summary_field(Fault::UnknownSummaryField)]
fn rejects_invalid_expectations(#[case] fault: Fault) {
    let project = Project::new();
    for entry in fs::read_dir(fixture_root()).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), project.0.join(entry.file_name())).unwrap();
    }
    let path = project.0.join("unused-binding.warn-json.stdout");
    let bytes = fs::read(&path).unwrap();
    let mut records: Vec<Value> = std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    match fault {
        Fault::MissingSource => fs::remove_file(project.0.join("unused-binding.kgr")).unwrap(),
        Fault::MissingStdout => fs::remove_file(&path).unwrap(),
        Fault::MissingExit => {
            fs::remove_file(project.0.join("unused-binding.warn-json.exit")).unwrap()
        }
        Fault::Orphan => fs::write(project.0.join("orphan.stderr"), b"").unwrap(),
        Fault::DuplicateField | Fault::DuplicateType | Fault::DuplicateSummaryField => {
            let (field, duplicate) = match fault {
                Fault::DuplicateField => ("\"code\":", "\"code\":\"forged\",\"code\":"),
                Fault::DuplicateType => (
                    "\"type\":\"diagnostic\"",
                    "\"type\":\"diagnostic\",\"type\":\"diagnostic\"",
                ),
                Fault::DuplicateSummaryField => {
                    ("\"exit_code\":0", "\"exit_code\":0,\"exit_code\":0")
                }
                _ => unreachable!(),
            };
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text.contains(field));
            // Inject duplicates into raw JSON: Value parsing would collapse equal keys.
            fs::write(&path, text.replacen(field, duplicate, 1)).unwrap();
        }
        Fault::PositionalObject(pointer, fields) => {
            let path = project.0.join("required-error.json.stdout");
            let text = fs::read_to_string(&path).unwrap();
            let mut values: Vec<Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let mut actual = values.clone();
            actual[0]["location"]["path"] = json!("/root/script.kgr");
            actual[0]["labels"][0]["location"]["path"] = json!("/root/script.kgr");
            let object = values[0].pointer_mut(pointer).unwrap();
            let positional = fields.iter().map(|field| object[*field].clone()).collect();
            *object = Value::Array(positional);
            let malformed = encode(&values);
            fs::write(path, &malformed).unwrap();
            let source = fs::read_to_string(project.0.join("required-error.kgr")).unwrap();
            assert!(
                wire::compare(&encode(&actual), &malformed, "/root", &source, 1).is_err(),
                "accepted {fault:?} as equal to valid object-shaped producer records"
            );
        }
        Fault::BlankLine => fs::write(&path, [bytes.as_slice(), b"\n"].concat()).unwrap(),
        Fault::InvalidExit => {
            fs::write(project.0.join("unused-binding.warn-json.exit"), b"00\n").unwrap()
        }
        Fault::InvalidUtf8 => fs::write(&path, b"\xff\n").unwrap(),
        Fault::MissingLabelLocation => {
            let path = project.0.join("required-error.json.stdout");
            let bytes = fs::read_to_string(&path).unwrap();
            let mut values: Vec<Value> = bytes
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            values[0]["labels"][0]
                .as_object_mut()
                .unwrap()
                .remove("location");
            fs::write(path, encode(&values)).unwrap();
        }
        other => {
            match other {
                Fault::UnknownField => records[0]["unexpected"] = json!(true),
                Fault::UnknownSummaryField => records[1]["unexpected"] = json!(true),
                Fault::MissingField => {
                    records[0].as_object_mut().unwrap().remove("location");
                }
                Fault::WrongType => records[0]["location"]["revision"] = json!("1"),
                Fault::InvalidRange => records[0]["location"]["byte_start"] = json!(100),
                Fault::Revision => records[0]["location"]["revision"] = json!(2),
                Fault::MissingSummary => {
                    records.pop();
                }
                Fault::SummaryOrder => records.reverse(),
                _ => unreachable!(),
            }
            fs::write(&path, encode(&records)).unwrap();
        }
    }
    assert!(
        fixtures::inventory(&project.0).is_err(),
        "accepted {fault:?}"
    );
}

fn encode(records: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).unwrap();
        bytes.push(b'\n');
    }
    bytes
}

#[test]
fn path_mapping_preserves_messages_notes_and_physical_text() {
    let input = b"message /root/script.kgr \\ \\r\\n\r\n  --> /root/script.kgr:1:15\n   | /root/script.kgr \\ \r\n";
    let expected = b"message /root/script.kgr \\ \\r\\n\r\n  --> $FIXTURE/script.kgr:1:15\n   | /root/script.kgr \\ \r\n";
    assert_eq!(wire::human(input, "/root").unwrap(), expected);
    assert!(wire::human(b"  --> /root-other/script.kgr:1:15\n", "/root").is_err());
    let source = fs::read_to_string(fixture_root().join("unused-binding.kgr")).unwrap();
    let text = fs::read_to_string(fixture_root().join("unused-binding.warn-json.stdout")).unwrap();
    let mut expected: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    expected[0]["message"] = json!("/root/script.kgr \\ arbitrary text\r\n");
    expected[0]["notes"] = json!(["/root/script.kgr \\ not a path field"]);
    let mut actual = expected.clone();
    actual[0]["location"]["path"] = json!("/root/script.kgr");
    wire::compare(&encode(&actual), &encode(&expected), "/root", &source, 0).unwrap();
    actual[0]["message"] = json!("$FIXTURE/script.kgr \\ arbitrary text\r\n");
    assert!(wire::compare(&encode(&actual), &encode(&expected), "/root", &source, 0).is_err());
    actual[0]["location"]["path"] = json!("/root-other/script.kgr");
    assert!(wire::compare(&encode(&actual), &encode(&expected), "/root", &source, 0).is_err());
}
