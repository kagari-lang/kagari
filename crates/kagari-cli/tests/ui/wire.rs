use kagari_source::{line_index::PositionEncoding, source::SourceFile};
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor, value::MapAccessDeserializer},
};
use std::{fmt, marker::PhantomData};

// Serde structs also accept positional sequences. CLI wire locations and labels are objects;
// delegate their fields through MapAccess to keep derive's duplicate/unknown/required checks.
struct Object<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(ObjectVisitor(PhantomData))
    }
}

struct ObjectVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
    type Value = Object<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a CLI diagnostic object")
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        T::deserialize(MapAccessDeserializer::new(map)).map(Object)
    }
}

fn object<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<T, D::Error> {
    Object::<T>::deserialize(deserializer).map(|object| object.0)
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Position {
    line: usize,
    column: usize,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Location {
    path: String,
    revision: u64,
    byte_start: usize,
    byte_end: usize,
    #[serde(deserialize_with = "object")]
    start: Position,
    #[serde(deserialize_with = "object")]
    end: Position,
}

fn nullable<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Location>, D::Error> {
    Option::<Object<Location>>::deserialize(deserializer)
        .map(|location| location.map(|location| location.0))
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Label {
    #[serde(deserialize_with = "nullable")]
    location: Option<Location>,
    message: String,
}

fn labels<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Label>, D::Error> {
    Vec::<Object<Label>>::deserialize(deserializer)
        .map(|labels| labels.into_iter().map(|label| label.0).collect())
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum Record {
    Diagnostic {
        context: String,
        code: String,
        severity: String,
        message: String,
        #[serde(deserialize_with = "nullable")]
        location: Option<Location>,
        #[serde(deserialize_with = "labels")]
        labels: Vec<Label>,
        notes: Vec<String>,
    },
    Summary {
        command: String,
        succeeded: usize,
        failed: usize,
        incomplete: usize,
        exit_code: u8,
    },
}

fn decode(bytes: &[u8]) -> Result<Vec<Record>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    if !text.ends_with('\n') {
        return Err("JSON stream must end with LF".into());
    }
    text.split_terminator('\n')
        .map(|line| {
            if line.is_empty() {
                return Err("blank JSON record".into());
            }
            serde_json::from_str(line).map_err(|e| e.to_string())
        })
        .collect()
}

fn validate(records: &[Record], source: &str, exit: i32) -> Result<(), String> {
    let Some((
        Record::Summary {
            command, exit_code, ..
        },
        diagnostics,
    )) = records.split_last()
    else {
        return Err("missing terminal summary".into());
    };
    if command != "lint" || i32::from(*exit_code) != exit || diagnostics.is_empty() {
        return Err("wrong command, summary exit or missing diagnostic".into());
    }
    let source = SourceFile::new("script.kgr", source);
    let check = |location: &Option<Location>| -> Result<(), String> {
        if let Some(location) = location {
            if location.path != "$FIXTURE/script.kgr" || location.revision != 1 {
                return Err("unexpected source path or revision".into());
            }
            if source
                .text()
                .get(location.byte_start..location.byte_end)
                .is_none()
            {
                return Err("invalid source byte range".into());
            }
            for (byte, expected) in [
                (location.byte_start, &location.start),
                (location.byte_end, &location.end),
            ] {
                let actual = source
                    .position(byte, PositionEncoding::Utf8)
                    .ok_or("invalid source position")?;
                if (actual.line, actual.character) != (expected.line, expected.column) {
                    return Err("source position does not match byte range".into());
                }
            }
        }
        Ok(())
    };
    for record in diagnostics {
        let Record::Diagnostic {
            context,
            code,
            severity,
            location,
            labels,
            ..
        } = record
        else {
            return Err("summary before the final record".into());
        };
        if context != "script.kgr"
            || code.is_empty()
            || !matches!(severity.as_str(), "error" | "warning")
        {
            return Err("invalid diagnostic context/code/severity".into());
        }
        check(location)?;
        for label in labels {
            check(&label.location)?;
        }
    }
    Ok(())
}

fn map_paths(records: &mut [Record], root: &str) -> Result<(), String> {
    let expected = format!("{root}/script.kgr");
    let map = |location: &mut Option<Location>| -> Result<(), String> {
        if let Some(location) = location {
            if location.path != expected {
                return Err(format!("unreviewed diagnostic path: {:?}", location.path));
            }
            location.path = "$FIXTURE/script.kgr".into();
        }
        Ok(())
    };
    for record in records {
        if let Record::Diagnostic {
            location, labels, ..
        } = record
        {
            map(location)?;
            for label in labels {
                map(&mut label.location)?;
            }
        }
    }
    Ok(())
}

pub(super) fn expected(bytes: &[u8], source: &str, exit: i32) -> Result<(), String> {
    validate(&decode(bytes)?, source, exit)
}

pub(super) fn compare(
    actual: &[u8],
    expected: &[u8],
    root: &str,
    source: &str,
    exit: i32,
) -> Result<(), String> {
    let mut actual = decode(actual)?;
    map_paths(&mut actual, root)?;
    validate(&actual, source, exit)?;
    let expected = decode(expected)?;
    validate(&expected, source, exit)?;
    if actual != expected {
        return Err(format!(
            "JSON records differ\nactual: {actual:#?}\nexpected: {expected:#?}"
        ));
    }
    Ok(())
}

pub(super) fn human(bytes: &[u8], root: &str) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let expected = format!("{root}/script.kgr");
    let mut result = String::new();
    for line in text.split_inclusive('\n') {
        if let Some(location) = line.strip_prefix("  --> ") {
            let (path_line, column) = location.rsplit_once(':').ok_or("missing display column")?;
            let (path, number) = path_line.rsplit_once(':').ok_or("missing display line")?;
            if path != expected
                || number.parse::<usize>().is_err()
                || column
                    .strip_suffix('\n')
                    .unwrap_or(column)
                    .parse::<usize>()
                    .is_err()
            {
                return Err("unreviewed human source location".into());
            }
            result.push_str("  --> $FIXTURE/script.kgr:");
            result.push_str(number);
            result.push(':');
            result.push_str(column);
        } else {
            result.push_str(line);
        }
    }
    Ok(result.into_bytes())
}
