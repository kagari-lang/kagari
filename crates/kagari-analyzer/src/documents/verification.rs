//! FZ03: bounded protocol operations checked against independent plain strings.
use crate::{documents::Documents, server::options::SessionLimits};
use kagari_source::{identity::FileId, source::SourceFile, source_database::SourceDatabase};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, Position,
    Range, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    VersionedTextDocumentIdentifier,
};
use std::sync::Arc;

mod coordinates;

const NAMES: [&str; 3] = [
    "c:/fuzz/a b.kgr",
    "untitled:Second",
    "memory://Third?view=1#body",
];
const URIS: [&str; 3] = [
    "file:///C:/fuzz/a%20b.kgr",
    "untitled:Second",
    "memory://Third?view=1#body",
];

struct Open {
    uri: String,
    version: i32,
    incarnation: u64,
    text: Option<String>,
}

struct Model {
    open: [Option<Open>; 3],
    visible: [Option<String>; 3],
    file_revisions: [u64; 3],
    revision: u64,
    incarnation: u64,
    file_limit: usize,
    total_limit: usize,
}

impl Model {
    fn bytes(&self) -> usize {
        self.open
            .iter()
            .flatten()
            .filter_map(|document| document.text.as_ref())
            .map(String::len)
            .sum()
    }

    fn available(&self, index: usize, text: &str) -> bool {
        let other_bytes: usize = self
            .open
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .filter_map(|(_, document)| document.as_ref()?.text.as_ref())
            .map(String::len)
            .sum();
        text.len() <= self.file_limit && other_bytes + text.len() <= self.total_limit
    }

    fn publish(&mut self, index: usize, text: Option<String>) {
        let visible = text;
        if self.visible[index] != visible {
            self.revision += 1;
            self.file_revisions[index] = self.revision;
            self.visible[index] = visible;
        }
    }

    fn open(&mut self, index: usize, uri: &str, version: i32, text: &str, language: &str) -> bool {
        if self.open[index].is_some() || language != "kagari" {
            return false;
        }
        let available = self.available(index, text);
        let text = available.then(|| text.to_owned());
        self.incarnation += 1;
        self.publish(index, text.clone());
        self.open[index] = Some(Open {
            uri: uri.into(),
            version,
            incarnation: self.incarnation,
            text,
        });
        available
    }

    fn change(&mut self, index: usize, version: i32, text: Option<&str>, invalid: bool) -> bool {
        let Some(document) = &self.open[index] else {
            return false;
        };
        if version <= document.version || invalid {
            return false;
        }
        let replacement = text.map(|text| self.available(index, text).then(|| text.to_owned()));
        let available = replacement.as_ref().is_none_or(Option::is_some);
        if let Some(text) = replacement.as_ref() {
            self.publish(index, text.clone());
        }
        let document = self.open[index].as_mut().unwrap();
        document.version = version;
        if let Some(text) = replacement {
            document.text = text;
        }
        available
    }

    fn close(&mut self, index: usize) -> bool {
        if self.open[index].take().is_none() {
            return false;
        }
        self.publish(index, None);
        true
    }
}

struct Retained {
    file: Arc<SourceFile>,
    text: String,
    id: FileId,
    revision: u64,
}

#[derive(Default)]
struct Observations {
    retained: Vec<Retained>,
    identities: [Option<FileId>; 3],
    retired: Vec<FileId>,
}

fn inspect(documents: &Documents, model: &Model, observations: &mut Observations, probe: u8) {
    let inputs = documents.inputs();
    assert_eq!(inputs.len(), model.open.iter().flatten().count());
    assert!(
        inputs
            .windows(2)
            .all(|pair| pair[0].uri.as_str() < pair[1].uri.as_str())
    );
    assert_eq!(documents.bytes, model.bytes());
    assert_eq!(
        documents.metadata_bytes,
        model
            .open
            .iter()
            .enumerate()
            .filter_map(|(index, open)| {
                open.as_ref()
                    .map(|open| NAMES[index].len() + open.uri.len())
            })
            .sum::<usize>()
    );
    assert!(!documents.synchronization_failed());
    let snapshot = documents.source.snapshot();
    assert_eq!(snapshot.revision().0, model.revision);
    for (index, document) in model.open.iter().enumerate() {
        let id = documents.source.file_id(NAMES[index]);
        if document.is_none() {
            assert_eq!(id, None, "closed document identity must be forgotten");
            if let Some(known) = observations.identities[index].take() {
                observations.retired.push(known);
            }
        } else if let Some(known) = observations.identities[index] {
            assert_eq!(
                id,
                Some(known),
                "source identity within one open incarnation"
            );
        } else {
            assert!(
                id.is_none_or(|id| !observations.retired.contains(&id)),
                "reopened physical identity was reused"
            );
            observations.identities[index] = id;
        }
        let source = id.and_then(|id| snapshot.file(id));
        assert_eq!(
            source.map(|file| file.text()),
            model.visible[index].as_deref()
        );
        if let Some(source) = source {
            assert_eq!(source.revision().0, model.file_revisions[index]);
        }
        let Some(document) = document else {
            continue;
        };
        let input = inputs
            .iter()
            .find(|input| input.source_name == NAMES[index])
            .unwrap();
        assert_eq!(input.uri.as_str(), document.uri);
        assert_eq!(input.version, document.version);
        assert_eq!(input.incarnation, document.incarnation);
        assert_eq!(input.unavailable, document.text.is_none());
        assert_eq!(
            input.source.as_ref().map(|file| file.text()),
            document.text.as_deref()
        );
        if let Some(source) = &input.source {
            assert_eq!(source.id(), id.unwrap());
            assert_eq!(source.name(), NAMES[index]);
            coordinates::check(source, document.text.as_ref().unwrap(), probe);
            // Bound retained lifetime coverage independently of input and operation counts.
            if observations.retained.len() < 8
                && !observations
                    .retained
                    .iter()
                    .any(|old| Arc::ptr_eq(&old.file, source))
            {
                observations.retained.push(Retained {
                    file: source.clone(),
                    text: document.text.as_ref().unwrap().clone(),
                    id: source.id(),
                    revision: model.file_revisions[index],
                });
            }
        }
    }
    for old in &observations.retained {
        assert_eq!(old.file.id(), old.id);
        assert_eq!(old.file.revision().0, old.revision);
        coordinates::check(&old.file, &old.text, probe);
    }
}

fn full(text: &str) -> TextDocumentContentChangeEvent {
    TextDocumentContentChangeEvent {
        range: None,
        range_length: None,
        text: text.into(),
    }
}

/// Two limit bytes precede up to 96 operation records: operation, URI, version,
/// payload length, then UTF-8-lossy payload. Limits cap source text at 128 bytes.
pub(crate) fn check(input: &[u8]) {
    if input.len() > 64 * 1024 {
        return;
    }
    let file_limit = usize::from(input.first().copied().unwrap_or(127) % 128) + 1;
    let total_limit = file_limit + usize::from(input.get(1).copied().unwrap_or(127) % 128);
    let source = SourceDatabase::new("c:/fuzz").unwrap();
    let mut documents = Documents::new(
        source,
        SessionLimits {
            open_file_bytes: file_limit,
            open_total_bytes: total_limit,
            ..SessionLimits::default()
        },
    );
    let mut model = Model {
        open: [None, None, None],
        visible: [None, None, None],
        file_revisions: [0, 0, 0],
        revision: 0,
        incarnation: 0,
        file_limit,
        total_limit,
    };
    let mut observations = Observations::default();
    let mut remaining = input.get(2..).unwrap_or_default();
    inspect(&documents, &model, &mut observations, 0);
    for _ in 0..96 {
        let Some(header) = remaining.get(..4) else {
            break;
        };
        let length = usize::from(header[3]).min(remaining.len() - 4);
        let text = String::from_utf8_lossy(&remaining[4..4 + length]);
        let operation = header[0] % 9;
        let index = usize::from(header[1] % 3);
        let uri = if index == 0 && header[1] & 0x80 != 0 {
            "file://localhost/c:/fuzz/a%20b.kgr"
        } else {
            URIS[index]
        };
        let version = i32::from(header[2] as i8);
        let expected;
        let actual;
        match operation {
            0 | 8 => {
                let language = if operation == 8 { "other" } else { "kagari" };
                expected = model.open(index, uri, version, &text, language);
                actual = documents.open(DidOpenTextDocumentParams {
                    text_document: TextDocumentItem::new(
                        uri.parse().unwrap(),
                        language.into(),
                        version,
                        text.into_owned(),
                    ),
                });
            }
            5 => {
                expected = model.close(index);
                actual = documents.close(DidCloseTextDocumentParams {
                    text_document: TextDocumentIdentifier::new(uri.parse().unwrap()),
                });
            }
            6 => {
                inspect(&documents, &model, &mut observations, header[2]);
                remaining = &remaining[4 + length..];
                continue;
            }
            _ => {
                let invalid = operation == 3 || operation == 7;
                expected = model.change(index, version, (operation != 4).then_some(&text), invalid);
                let content_changes = match operation {
                    2 => vec![full("discarded"), full(&text)],
                    3 | 7 => {
                        let mut invalid = full(&text);
                        if operation == 3 {
                            invalid.range =
                                Some(Range::new(Position::new(0, 0), Position::new(0, 1)));
                        } else {
                            invalid.range_length = Some(1);
                        }
                        vec![full("must not commit"), invalid]
                    }
                    4 => Vec::new(),
                    _ => vec![full(&text)],
                };
                actual = documents.change(DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier::new(
                        uri.parse().unwrap(),
                        version,
                    ),
                    content_changes,
                });
            }
        }
        assert_eq!(
            actual.is_ok(),
            expected,
            "operation {operation}, document {index}: {actual:?}"
        );
        inspect(&documents, &model, &mut observations, header[2]);
        remaining = &remaining[4 + length..];
    }
}
