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
    assert!(!docs.open[uri].available);
    assert!(docs.source.snapshot().file(id).is_none());
    assert_eq!(docs.bytes, 0);
    assert_eq!(retained.file(id).unwrap().text(), "1234");
    docs.change(change(uri, 3, vec![full("ok")])).unwrap();
    assert!(docs.open[uri].available);
    assert_eq!(docs.source.snapshot().file(id).unwrap().text(), "ok");
    docs.open(open("untitled:other.kgr", 1, "1234")).unwrap();
    assert!(docs.change(change(uri, 4, vec![full("123")])).is_err());
    assert_eq!(docs.bytes, 4);
    docs.close(close(uri)).unwrap();
    assert_eq!(docs.bytes, 4);
}

#[test]
fn closing_retires_current_identity_but_preserves_retained_inputs() {
    let mut docs = documents();
    let uri = "file:///C:/project/a%20b.kgr";
    let name = "c:/project/a b.kgr";
    docs.open(open(uri, 8, "你好😀\r\n")).unwrap();
    let capture = docs.inputs().pop().unwrap();
    assert_eq!(capture.uri.as_str(), uri);
    assert_eq!(capture.source_name, name);
    assert_eq!(capture.version, 8);
    assert!(!capture.unavailable);
    let id = docs.source.file_id(name).unwrap();
    let retained = docs.source.snapshot();
    docs.change(change(uri, 10, vec![full("fn {")])).unwrap();
    let edited = docs.source.snapshot();
    docs.close(close(uri)).unwrap();
    assert!(docs.source.file_id(name).is_none());
    assert!(docs.source.snapshot().file(id).is_none());
    assert_eq!(docs.metadata_bytes, 0);
    assert_eq!(retained.file(id).unwrap().text(), "你好😀\r\n");
    assert_eq!(edited.file(id).unwrap().text(), "fn {");
    docs.open(open(uri, 1, "")).unwrap();
    assert!(docs.inputs()[0].incarnation > capture.incarnation);
    assert_eq!(capture.source.unwrap().text(), "你好😀\r\n");
    let reopened = docs.source.file_id(name).unwrap();
    assert_ne!(reopened, id);
    assert_eq!(docs.source.snapshot().file(reopened).unwrap().text(), "");
    docs.change(change(uri, 2, vec![full("new lifecycle")]))
        .unwrap();
}

#[test]
fn unavailable_inputs_preserve_stamps_without_fabricated_text_or_lifecycle_reuse() {
    let mut docs = documents();
    docs.limits.open_file_bytes = 4;
    let uri = "file:///C:/project/a%20b.kgr";
    let name = "c:/project/a b.kgr";
    assert!(docs.open(open(uri, 1, "oversized")).is_err());
    assert!(docs.source.file_id(name).is_none());
    let unavailable = docs.inputs().pop().unwrap();
    assert!(unavailable.unavailable);
    assert!(unavailable.source.is_none());
    assert_eq!(unavailable.source_name, name);
    assert_eq!(unavailable.uri.as_str(), uri);
    assert_eq!(unavailable.version, 1);
    docs.change(change(uri, 2, vec![full("good")])).unwrap();
    let retained = docs.inputs().pop().unwrap();
    assert_eq!(retained.incarnation, unavailable.incarnation);
    assert_eq!(retained.source.as_ref().unwrap().text(), "good");
    assert!(
        docs.change(change(uri, 3, vec![full("oversized")]))
            .is_err()
    );
    assert!(docs.inputs()[0].source.is_none());
    assert!(docs.inputs()[0].unavailable);
    assert_eq!(docs.inputs()[0].version, 3);
    assert_eq!(retained.source.unwrap().text(), "good");
    assert!(!docs.synchronization_failed());
    docs.close(close(uri)).unwrap();
    docs.incarnation = u64::MAX;
    let before = docs.source.snapshot();
    assert_eq!(
        docs.open(open(uri, 1, "new")).unwrap_err(),
        "document incarnation exhausted"
    );
    assert!(docs.inputs().is_empty());
    assert_eq!(docs.source.snapshot().revision(), before.revision());
    assert!(docs.source.file_id(name).is_none());
    assert_eq!(docs.metadata_bytes, 0);
    assert!(docs.synchronization_failed());
}

#[test]
fn metadata_overflow_latches_failure_without_accepting_unrepresented_open_buffers() {
    for (count, uri, metadata) in [
        (1, 8192, 256 * 1024),
        (1024, 10, 256 * 1024),
        (1024, 8192, 20),
    ] {
        let mut docs = documents();
        docs.limits.open_documents = count;
        docs.limits.document_uri_bytes = uri;
        docs.limits.document_metadata_bytes = metadata;
        docs.open(open("untitled:a", 1, "")).unwrap();
        let before = docs.source.snapshot();
        let retained = docs.inputs();
        assert!(docs.open(open("untitled:another", 1, "")).is_err());
        assert!(docs.synchronization_failed());
        assert!(docs.source.file_id("untitled:another").is_none());
        assert_eq!(docs.source.snapshot().revision(), before.revision());
        assert_eq!(docs.inputs().len(), retained.len());
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
