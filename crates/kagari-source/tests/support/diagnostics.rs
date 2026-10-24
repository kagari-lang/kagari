//! Frozen compiler-UI expectations and projection of real producer records.
//!
//! `kagari-source` owns diagnostic records and retained source identities. TO03's
//! syntax library harness imports this file with a narrow `#[path]`; TO04's HIR
//! harness will use the same schema/projection for unknown parameter/return types
//! and call-argument mismatches from its captured snapshot. Owners keep production,
//! fixture discovery, typed payloads and recovery assertions in their own harness.
//! This is test-only data, not a serialization API, renderer or global UI runner.

use kagari_source::{
    diagnostic::Severity, identity::FileSpan, presentation::DiagnosticRecord, source::SourceFile,
};
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor, value::MapAccessDeserializer},
};
use std::{fmt, marker::PhantomData};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedRecord {
    code: String,
    severity: ExpectedSeverity,
    // deserialize_with makes this nullable field required, unlike bare Option.
    #[serde(deserialize_with = "required_primary")]
    primary: Option<ExpectedSpan>,
    message: String,
    #[serde(deserialize_with = "required_objects")]
    labels: Vec<ExpectedLabel>,
    notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
enum ExpectedSeverity {
    Error,
    Warning,
}

impl TryFrom<String> for ExpectedSeverity {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        match value.as_str() {
            "error" => Ok(Self::Error),
            "warning" => Ok(Self::Warning),
            _ => Err(format!("unknown severity: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedSpan {
    path: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedLabel {
    #[serde(deserialize_with = "required_object")]
    span: ExpectedSpan,
    message: String,
}

fn required_primary<'de, D>(deserializer: D) -> Result<Option<ExpectedSpan>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Object<ExpectedSpan>>::deserialize(deserializer).map(|span| span.map(|span| span.0))
}

// Serde structs otherwise accept positional sequences, which are not UI objects.
// Retain derive's required/duplicate/unknown-field checks through MapAccess.
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
        formatter.write_str("a diagnostic object")
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        T::deserialize(MapAccessDeserializer::new(map)).map(Object)
    }
}

fn required_object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Object::<T>::deserialize(deserializer).map(|object| object.0)
}

fn required_objects<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Vec::<Object<T>>::deserialize(deserializer)
        .map(|objects| objects.into_iter().map(|object| object.0).collect())
}

/// Parse all required fields without defaults and validate logical UTF-8 ranges.
/// Arrays retain their original order, including explicitly empty arrays.
pub fn read_expectations(
    text: &str,
    sources: &[&SourceFile],
) -> Result<Vec<ExpectedRecord>, String> {
    let objects: Vec<Object<ExpectedRecord>> =
        serde_json::from_str(text).map_err(|error| error.to_string())?;
    let records = objects
        .into_iter()
        .map(|object| object.0)
        .collect::<Vec<_>>();
    for record in &records {
        for span in record
            .primary
            .iter()
            .chain(record.labels.iter().map(|label| &label.span))
        {
            let mut matching = sources.iter().filter(|source| source.name() == span.path);
            let source = matching
                .next()
                .ok_or_else(|| format!("unknown logical source: {}", span.path))?;
            if matching.next().is_some() {
                return Err(format!("ambiguous logical source: {}", span.path));
            }
            if source.text().get(span.start..span.end).is_none() {
                return Err(format!("invalid UTF-8 range: {span:?}"));
            }
        }
    }
    Ok(records)
}

/// Validate every present identity/revision/range before replacing IDs with names.
/// No location is discarded or made anonymous on validation failure. Producers
/// that can lose invalid raw spans must be checked by their owner before adaptation.
pub fn project_records(
    records: &[DiagnosticRecord],
    sources: &[&SourceFile],
) -> Result<Vec<ExpectedRecord>, String> {
    records
        .iter()
        .map(|record| {
            Ok(ExpectedRecord {
                code: record.code.clone(),
                severity: match record.severity {
                    Severity::Error => ExpectedSeverity::Error,
                    Severity::Warning => ExpectedSeverity::Warning,
                },
                primary: record
                    .span
                    .map(|span| project_span(span, sources))
                    .transpose()?,
                message: record.message.clone(),
                labels: record
                    .labels
                    .iter()
                    .map(|label| {
                        Ok(ExpectedLabel {
                            span: project_span(label.span, sources)?,
                            message: label.message.clone(),
                        })
                    })
                    .collect::<Result<_, String>>()?,
                notes: record.notes.clone(),
            })
        })
        .collect()
}

fn project_span(span: FileSpan, sources: &[&SourceFile]) -> Result<ExpectedSpan, String> {
    let source = sources
        .iter()
        .find(|source| source.id() == span.file && source.revision() == span.revision)
        .ok_or_else(|| format!("uncaptured file/revision: {span:?}"))?;
    if source.span(span.range) != Some(span) {
        return Err(format!("invalid captured UTF-8 range: {span:?}"));
    }
    if sources
        .iter()
        .filter(|other| other.name() == source.name())
        .count()
        != 1
    {
        return Err(format!("ambiguous logical source: {}", source.name()));
    }
    Ok(ExpectedSpan {
        path: source.name().to_owned(),
        start: span.range.start,
        end: span.range.end,
    })
}
