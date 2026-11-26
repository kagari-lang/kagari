//! Open-document protocol metadata over source-owned identities, text and snapshots.
use crate::protocol::source_name;
use kagari_analyzer::workspace::EditorWorkspace;
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, Uri,
};
use std::collections::HashMap;

struct OpenDocument {
    uri: Uri,
    language_id: String,
    version: i32,
}

pub(crate) struct Documents {
    workspace: EditorWorkspace,
    // A generated display URI may outlive its binding in the current snapshot.
    open: HashMap<String, OpenDocument>,
}

impl Documents {
    pub(crate) fn new(workspace: EditorWorkspace) -> Self {
        Self {
            workspace,
            open: HashMap::new(),
        }
    }

    pub(crate) fn open(&mut self, params: DidOpenTextDocumentParams) -> Result<(), String> {
        let item = params.text_document;
        let name = source_name(&item.uri)?;
        if let Some(document) = self.open.get(&name) {
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
        self.workspace.open(&name, item.text)?;
        self.open.insert(
            name,
            OpenDocument {
                uri: item.uri,
                language_id: item.language_id,
                version: item.version,
            },
        );
        Ok(())
    }

    pub(crate) fn change(&mut self, params: DidChangeTextDocumentParams) -> Result<(), String> {
        let name = source_name(&params.text_document.uri)?;
        let document = self.open.get_mut(&name).ok_or("document is not open")?;

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
            self.workspace.change(&name, change.text)?;
        }
        document.version = params.text_document.version;
        Ok(())
    }

    pub(crate) fn close(&mut self, params: DidCloseTextDocumentParams) -> Result<(), String> {
        let name = source_name(&params.text_document.uri)?;
        if !self.open.contains_key(&name) {
            return Err("document is not open".into());
        }
        self.workspace.close(&name)?;
        self.open.remove(&name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::identity::ModuleIdentity;
    use kagari_source::{
        line_index::PositionEncoding,
        source_database::{CapturedSource, SourceDatabase, SourceLayer},
    };
    use lsp_types::{
        Position, Range, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
        VersionedTextDocumentIdentifier,
    };

    fn documents() -> Documents {
        Documents::new(EditorWorkspace::new(SourceDatabase::new("c:/project").unwrap()).unwrap())
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
    fn overlays_restore_base_or_disappear_and_reopen_preserves_identity() {
        for base in [None, Some("retained base")] {
            let mut sources = SourceDatabase::new("c:/project").unwrap();
            let uri = "file:///C:/project/a%20b.kgr";
            let name = "c:/project/a b.kgr";
            if let Some(text) = base {
                sources.set(name, text.into(), SourceLayer::Base).unwrap();
            }
            let mut docs = Documents::new(EditorWorkspace::new(sources).unwrap());
            docs.open(open(uri, 8, "你好😀\r\n")).unwrap();
            let id = docs.workspace.sources().file_id(name).unwrap();
            let retained = docs.workspace.sources().snapshot();
            docs.change(change(uri, 10, vec![full("fn {")])).unwrap();
            let edited = docs.workspace.sources().snapshot();
            assert_eq!(edited.file(id).unwrap().text(), "fn {");
            docs.close(close(uri)).unwrap();
            assert_eq!(
                docs.workspace
                    .sources()
                    .snapshot()
                    .file(id)
                    .map(|f| f.text()),
                base
            );
            assert_eq!(retained.file(id).unwrap().text(), "你好😀\r\n");
            assert_eq!(edited.file(id).unwrap().text(), "fn {");
            docs.open(open(uri, 1, "")).unwrap();
            assert_eq!(docs.workspace.sources().file_id(name), Some(id));
            assert_eq!(
                docs.workspace.sources().snapshot().file(id).unwrap().text(),
                ""
            );
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
        assert!(docs.workspace.sources().file_id(uri).is_none());
        docs.open(open(uri, 4, "before")).unwrap();
        let id = docs.workspace.sources().file_id(uri).unwrap();
        let before = docs.workspace.sources().snapshot();
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
            assert_eq!(
                docs.workspace.sources().snapshot().revision(),
                before.revision()
            );
            assert_eq!(
                docs.workspace.sources().snapshot().file(id).unwrap().text(),
                "before"
            );
        }
        docs.change(change(uri, 5, vec![full("intermediate"), full("after")]))
            .unwrap();
        assert_eq!(
            docs.workspace.sources().snapshot().file(id).unwrap().text(),
            "after"
        );
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
            let id = docs.workspace.sources().file_id(uri).unwrap();
            let retained = docs.workspace.sources().snapshot();
            let mut version = 1;
            for edited in [
                original.replace("\"#;", "\";"),
                original.replace("{bytes}", "🙂 {bytes}"),
                original.clone(),
            ] {
                version += 1;
                docs.change(change(uri, version, vec![full(&edited)]))
                    .unwrap();
                let snapshot = docs.workspace.sources().snapshot();
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
    fn open_generated_views_remain_read_only_until_closed_even_after_removal() {
        const KEY: &str = "kagari://native/host/api";
        const DISPLAY: &str = "file:///c:/cache/abc.kgr";
        let mut docs = documents();
        let cancel = Default::default();
        let generated = CapturedSource {
            key: KEY.into(),
            display_name: "c:/cache/abc.kgr".into(),
            module: ModuleIdentity::single_file(KEY),
            text: "// published".into(),
        };
        let snapshot = docs
            .workspace
            .replace_generated(vec![generated], Default::default(), &cancel)
            .unwrap();
        assert!(
            docs.open(open(DISPLAY, 1, "different bytes"))
                .unwrap_err()
                .contains("read-only")
        );
        for uri in [KEY, DISPLAY] {
            docs.open(open(uri, 1, "// published")).unwrap();
        }
        assert_eq!(
            docs.workspace.sources().snapshot().revision(),
            snapshot.revision()
        );
        docs.workspace
            .replace_generated(vec![], Default::default(), &cancel)
            .unwrap();
        for uri in [DISPLAY, KEY] {
            assert!(
                docs.change(change(uri, 2, vec![full("not an overlay")]))
                    .unwrap_err()
                    .contains("read-only")
            );
            docs.close(close(uri)).unwrap();
        }
        assert_eq!(docs.workspace.sources().snapshot().files().count(), 0);
    }

    #[test]
    fn protocol_versions_advance_independently_of_source_revisions() {
        let mut docs = documents();
        let uri = "memory://scratch/main.kgr?view=1#body";
        docs.open(open(uri, -1, "same")).unwrap();
        let before = docs.workspace.sources().snapshot().revision();
        docs.change(change(uri, 7, vec![full("same")])).unwrap();
        assert_eq!(docs.workspace.sources().snapshot().revision(), before);
        assert!(docs.change(change(uri, 7, vec![full("stale")])).is_err());
        docs.change(change(uri, 8, vec![])).unwrap();
        assert!(docs.change(change(uri, 8, vec![full("stale")])).is_err());
        assert_eq!(docs.workspace.sources().snapshot().revision(), before);
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
        assert_eq!(docs.workspace.sources().snapshot().files().count(), 0);
        let revision = docs.workspace.sources().snapshot().revision();
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
        assert_eq!(docs.workspace.sources().snapshot().revision(), revision);
        assert!(docs.workspace.sources().file_id("untitled:Other").is_none());
    }
}
