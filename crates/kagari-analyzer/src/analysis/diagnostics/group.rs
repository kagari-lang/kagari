//! Immutable diagnostic records prepared on the analysis worker for publication.
use lsp_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticTag, NumberOrString, Uri};
use serde_json::{Value, to_writer};
use std::{
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    io::{self, Error, Write},
    mem::size_of,
};

#[derive(Debug)]
pub(crate) struct DiagnosticRecord {
    diagnostic: Diagnostic,
    fingerprint: u64,
    retained_bytes: usize,
}

impl DiagnosticRecord {
    pub fn diagnostic(&self) -> &Diagnostic {
        &self.diagnostic
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

#[derive(Debug)]
pub(crate) struct DiagnosticGroup {
    uri: Uri,
    records: Vec<DiagnosticRecord>,
    retained_bytes: usize,
}

impl DiagnosticGroup {
    /// Hashes the complete wire record without retaining serialized JSON.
    /// Fingerprints accelerate lookup; consumers must still compare full records.
    pub fn new(uri: Uri, diagnostics: Vec<Diagnostic>) -> io::Result<Self> {
        // Reparse the exact URI to avoid retaining an oversized producer allocation.
        let uri: Uri = uri.as_str().parse().map_err(Error::other)?;
        let mut records = Vec::with_capacity(diagnostics.len());
        let mut retained_bytes = size_of::<Self>()
            + uri.as_str().len()
            + records.capacity() * size_of::<DiagnosticRecord>();
        for diagnostic in diagnostics {
            let mut fingerprint = HashWriter::default();
            to_writer(&mut fingerprint, &diagnostic).map_err(Error::other)?;
            let charge = diagnostic_bytes(&diagnostic);
            retained_bytes = retained_bytes
                .checked_add(charge)
                .ok_or_else(|| Error::other("diagnostic retained-byte counter exhausted"))?;
            records.push(DiagnosticRecord {
                diagnostic,
                fingerprint: fingerprint.0.finish(),
                retained_bytes: charge,
            });
        }
        Ok(Self {
            uri,
            records,
            retained_bytes,
        })
    }

    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    pub fn records(&self) -> &[DiagnosticRecord] {
        &self.records
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
}

#[derive(Default)]
struct HashWriter(DefaultHasher);

impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn diagnostic_bytes(diagnostic: &Diagnostic) -> usize {
    let mut bytes = size_of::<Diagnostic>() + diagnostic.message.capacity();
    bytes += diagnostic.source.as_ref().map_or(0, String::capacity);
    if let Some(NumberOrString::String(code)) = &diagnostic.code {
        bytes += code.capacity();
    }
    if let Some(description) = &diagnostic.code_description {
        bytes += description.href.as_str().len();
    }
    if let Some(tags) = &diagnostic.tags {
        bytes += tags.capacity() * size_of::<DiagnosticTag>();
    }
    if let Some(related) = &diagnostic.related_information {
        bytes += related.capacity() * size_of::<DiagnosticRelatedInformation>();
        for item in related {
            bytes += item.message.capacity() + item.location.uri.as_str().len();
        }
    }
    if let Some(data) = &diagnostic.data {
        bytes += json_bytes(data);
    }
    bytes
}

fn json_bytes(value: &Value) -> usize {
    let mut pending = vec![value];
    let mut bytes = 0;
    while let Some(value) = pending.pop() {
        bytes += size_of::<Value>();
        match value {
            Value::String(text) => bytes += text.capacity(),
            Value::Array(items) => {
                bytes += items.capacity() * size_of::<Value>();
                pending.extend(items);
            }
            Value::Object(items) => {
                // Charge key/value storage plus conservative tree-node overhead.
                bytes += items.len() * (size_of::<(String, Value)>() + 64);
                for (key, value) in items {
                    bytes += key.capacity();
                    pending.push(value);
                }
            }
            _ => {}
        }
    }
    bytes
}
