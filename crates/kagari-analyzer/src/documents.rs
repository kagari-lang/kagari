//! Open-document protocol metadata over source-owned identities, text and snapshots.
use crate::{protocol::source_name, server::options::SessionLimits};
use kagari_common::identity::ModuleIdentity;
use kagari_source::{
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, Uri,
};
use std::collections::HashMap;

struct OpenDocument {
    uri: Uri,
    language_id: String,
    version: i32,
    bytes: usize,
    available: bool,
}

pub(crate) struct Documents {
    source: SourceDatabase,
    open: HashMap<FileId, OpenDocument>,
    limits: SessionLimits,
    bytes: usize,
}

impl Documents {
    pub(crate) fn new(source: SourceDatabase, limits: SessionLimits) -> Self {
        Self {
            source,
            open: HashMap::new(),
            limits,
            bytes: 0,
        }
    }

    pub(crate) fn open(&mut self, params: DidOpenTextDocumentParams) -> Result<(), String> {
        let item = params.text_document;
        let name = source_name(&item.uri)?;
        if let Some(document) = self.source.file_id(&name).and_then(|id| self.open.get(&id)) {
            return Err(format!(
                "document already open as {} (language {})",
                document.uri.as_str(),
                document.language_id
            ));
        }
        if item.language_id != "kagari" {
            return Err(format!(
                "unsupported document language: {}",
                item.language_id
            ));
        }
        let length = item.text.len();
        let available = length <= self.limits.open_file_bytes
            && length <= self.limits.open_total_bytes.saturating_sub(self.bytes);
        let id = if available {
            self.source.set(&name, item.text, SourceLayer::Overlay)?
        } else {
            self.source
                .bind_module(&name, ModuleIdentity::single_file(name.clone()))?
        };
        let bytes = if available { length } else { 0 };
        self.bytes += bytes;
        self.open.insert(
            id,
            OpenDocument {
                uri: item.uri,
                language_id: item.language_id,
                version: item.version,
                bytes,
                available,
            },
        );
        if available {
            Ok(())
        } else {
            Err("document synchronization unavailable: source byte limit exceeded".into())
        }
    }

    pub(crate) fn change(&mut self, params: DidChangeTextDocumentParams) -> Result<(), String> {
        let name = source_name(&params.text_document.uri)?;
        let id = self.open_id(&name)?;
        let document = self.open.get_mut(&id).expect("validated open document");
        if params.text_document.version <= document.version {
            return Err(format!(
                "document version {} is not newer than {}",
                params.text_document.version, document.version
            ));
        }
        if params
            .content_changes
            .iter()
            .any(|change| change.range.is_some() || change.range_length.is_some())
        {
            return Err("full synchronization does not accept ranged changes".into());
        }
        // Validate the whole notification first. With full replacements only the last
        // text survives; no intermediate source state needs publication.
        if let Some(change) = params.content_changes.into_iter().last() {
            let remaining = self.bytes - document.bytes;
            let length = change.text.len();
            if length > self.limits.open_file_bytes
                || length > self.limits.open_total_bytes.saturating_sub(remaining)
            {
                self.source.close_overlay(&name)?;
                self.bytes = remaining;
                document.bytes = 0;
                document.available = false;
                document.version = params.text_document.version;
                return Err(
                    "document synchronization unavailable: source byte limit exceeded".into(),
                );
            }
            self.source.set(&name, change.text, SourceLayer::Overlay)?;
            self.bytes = remaining + length;
            document.bytes = length;
            document.available = true;
        }
        document.version = params.text_document.version;
        Ok(())
    }

    pub(crate) fn close(&mut self, params: DidCloseTextDocumentParams) -> Result<(), String> {
        let name = source_name(&params.text_document.uri)?;
        let id = self.open_id(&name)?;
        self.source.close_overlay(&name)?;
        if let Some(document) = self.open.remove(&id) {
            self.bytes -= document.bytes;
        }
        Ok(())
    }

    fn open_id(&self, name: &str) -> Result<FileId, String> {
        self.source
            .file_id(name)
            .filter(|id| self.open.contains_key(id))
            .ok_or_else(|| "document is not open".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_source::line_index::PositionEncoding;
    use lsp_types::{
        Position, Range, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
        VersionedTextDocumentIdentifier,
    };

    fn documents() -> Documents {
        Documents::new(
            SourceDatabase::new("c:/project").unwrap(),
            SessionLimits::default(),
        )
    }

    fn open(uri: &str, version: i32, text: &str) -> DidOpenTextDocumentParams {
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(
                uri.parse().unwrap(),
                "kagari".into(),
                version,
                text.into(),
            ),
        }
    }

    fn full(text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: text.into(),
        }
    }

    fn change(
        uri: &str,
        version: i32,
        content_changes: Vec<TextDocumentContentChangeEvent>,
    ) -> DidChangeTextDocumentParams {
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier::new(uri.parse().unwrap(), version),
            content_changes,
        }
    }

    fn close(uri: &str) -> DidCloseTextDocumentParams {
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier::new(uri.parse().unwrap()),
        }
    }

    #[test]
    fn oversized_sync_releases_text_and_recovers_with_a_later_full_change() {
        let mut docs = documents();
        docs.limits.open_file_bytes = 4;
        docs.limits.open_total_bytes = 6;
        let uri = "untitled:large.kgr";
        docs.open(open(uri, 1, "1234")).unwrap();
        let retained = docs.source.snapshot();
        let id = docs.source.file_id(uri).unwrap();
        assert!(docs.change(change(uri, 2, vec![full("12345")])).is_err());
        assert!(!docs.open[&id].available);
        assert!(docs.source.snapshot().file(id).is_none());
        assert_eq!(docs.bytes, 0);
        assert_eq!(retained.file(id).unwrap().text(), "1234");
        docs.change(change(uri, 3, vec![full("ok")])).unwrap();
        assert!(docs.open[&id].available);
        assert_eq!(docs.source.snapshot().file(id).unwrap().text(), "ok");
        docs.open(open("untitled:other.kgr", 1, "1234")).unwrap();
        assert!(docs.change(change(uri, 4, vec![full("123")])).is_err());
        assert_eq!(docs.bytes, 4);
        docs.close(close(uri)).unwrap();
        assert_eq!(docs.bytes, 4);
    }

    #[test]
    fn overlays_restore_base_or_disappear_and_reopen_preserves_identity() {
        for base in [None, Some("retained base")] {
            let mut docs = documents();
            let uri = "file:///C:/project/a%20b.kgr";
            let name = "c:/project/a b.kgr";
            if let Some(text) = base {
                docs.source
                    .set(name, text.into(), SourceLayer::Base)
                    .unwrap();
            }
            docs.open(open(uri, 8, "你好😀\r\n")).unwrap();
            let id = docs.source.file_id(name).unwrap();
            let retained = docs.source.snapshot();
            docs.change(change(uri, 10, vec![full("fn {")])).unwrap();
            let edited = docs.source.snapshot();
            assert_eq!(edited.file(id).unwrap().text(), "fn {");
            docs.close(close(uri)).unwrap();
            assert_eq!(docs.source.snapshot().file(id).map(|f| f.text()), base);
            assert_eq!(retained.file(id).unwrap().text(), "你好😀\r\n");
            assert_eq!(edited.file(id).unwrap().text(), "fn {");
            docs.open(open(uri, 1, "")).unwrap();
            assert_eq!(docs.source.file_id(name), Some(id));
            assert_eq!(docs.source.snapshot().file(id).unwrap().text(), "");
            docs.change(change(uri, 2, vec![full("new lifecycle")]))
                .unwrap();
        }
    }

    #[test]
    fn invalid_notifications_do_not_partially_commit_text_or_versions() {
        let mut docs = documents();
        let uri = "untitled:Untitled-1";
        assert!(docs.change(change(uri, 1, vec![full("unopened")])).is_err());
        assert!(docs.close(close(uri)).is_err());
        assert!(docs.source.file_id(uri).is_none());
        docs.open(open(uri, 4, "before")).unwrap();
        let id = docs.source.file_id(uri).unwrap();
        let before = docs.source.snapshot();
        assert!(docs.open(open(uri, 99, "duplicate")).is_err());
        for version in [3, 4] {
            assert!(
                docs.change(change(uri, version, vec![full("stale")]))
                    .is_err()
            );
        }
        let mut ranged = full("ranged");
        ranged.range = Some(Range::new(Position::new(0, 0), Position::new(0, 1)));
        let mut length_only = full("length only");
        length_only.range_length = Some(1);
        for invalid in [ranged, length_only] {
            assert!(
                docs.change(change(uri, 5, vec![full("partial"), invalid]))
                    .is_err()
            );
            assert_eq!(docs.source.snapshot().revision(), before.revision());
            assert_eq!(docs.source.snapshot().file(id).unwrap().text(), "before");
        }
        docs.change(change(uri, 5, vec![full("intermediate"), full("after")]))
            .unwrap();
        assert_eq!(docs.source.snapshot().file(id).unwrap().text(), "after");
        assert_eq!(before.file(id).unwrap().text(), "before");
    }

    #[test]
    fn multiline_text_edits_preserve_physical_offsets_and_retained_revisions() {
        for eol in ["\n", "\r\n"] {
            let mut docs = documents();
            let uri = "untitled:strings.kgr";
            let original = [
                "fn f() { val text = r#\"",
                "🙂 {literal} \\n",
                "\"#; val cooked = \"",
                "🙂é {name:?}",
                "\"; val byte = b'\\xFF'; val bytes = b\"",
                "{bytes} \\x00\\xFF\\",
                "  continued\"; val raw_bytes = br#\"",
                "{raw_bytes} \\xFF",
                "\"#; }",
            ]
            .join(eol);
            docs.open(open(uri, 1, &original)).unwrap();
            let id = docs.source.file_id(uri).unwrap();
            let retained = docs.source.snapshot();
            let mut version = 1;
            for edited in [
                original.replace("\"#;", "\";"),
                original.replace("{bytes}", "🙂 {bytes}"),
                original.clone(),
            ] {
                version += 1;
                docs.change(change(uri, version, vec![full(&edited)]))
                    .unwrap();
                let snapshot = docs.source.snapshot();
                let file = snapshot.file(id).unwrap();
                assert_eq!(file.text(), edited);
                let offset = edited.find("{name").unwrap();
                let position = file.position(offset, PositionEncoding::Utf16).unwrap();
                assert_eq!(position.line, 3);
                assert_eq!(position.character, 4);
                assert_eq!(file.offset(position, PositionEncoding::Utf16), Some(offset));
                let byte_offset = edited.find("{bytes}").unwrap();
                let byte_position = file.position(byte_offset, PositionEncoding::Utf16).unwrap();
                assert_eq!(byte_position.line, 5);
                assert_eq!(
                    byte_position.character,
                    if edited.contains("🙂 {bytes}") {
                        3
                    } else {
                        0
                    }
                );
                assert_eq!(
                    file.offset(byte_position, PositionEncoding::Utf16),
                    Some(byte_offset)
                );
                assert_eq!(retained.file(id).unwrap().text(), original);
            }
        }
    }

    #[test]
    fn protocol_versions_advance_independently_of_source_revisions() {
        let mut docs = documents();
        let uri = "memory://scratch/main.kgr?view=1#body";
        docs.open(open(uri, -1, "same")).unwrap();
        let before = docs.source.snapshot().revision();
        docs.change(change(uri, 7, vec![full("same")])).unwrap();
        assert_eq!(docs.source.snapshot().revision(), before);
        assert!(docs.change(change(uri, 7, vec![full("stale")])).is_err());
        docs.change(change(uri, 8, vec![])).unwrap();
        assert!(docs.change(change(uri, 8, vec![full("stale")])).is_err());
        assert_eq!(docs.source.snapshot().revision(), before);
    }

    #[test]
    fn aliases_share_open_state_and_rejected_inputs_create_no_sources() {
        let mut docs = documents();
        docs.open(open("file:///C:/project/a%20b.kgr", 1, "before"))
            .unwrap();
        assert!(
            docs.open(open("file://localhost/c:/project/a%20b.kgr", 2, "alias"))
                .is_err()
        );
        docs.change(change(
            "file:///c:/project/a%20b.kgr",
            2,
            vec![full("after")],
        ))
        .unwrap();
        docs.close(close("file://localhost/C:/project/a%20b.kgr"))
            .unwrap();
        assert_eq!(docs.source.snapshot().files().count(), 0);
        let revision = docs.source.snapshot().revision();
        for uri in [
            "file://server/share/a.kgr",
            "file:////server/share/a.kgr",
            "file:///C:/a.kgr?query",
        ] {
            assert!(docs.open(open(uri, 1, "rejected")).is_err());
        }
        let mut wrong_language = open("untitled:Other", 1, "");
        wrong_language.text_document.language_id = "rust".into();
        assert!(docs.open(wrong_language).is_err());
        assert_eq!(docs.source.snapshot().revision(), revision);
        assert!(docs.source.file_id("untitled:Other").is_none());
    }
}
