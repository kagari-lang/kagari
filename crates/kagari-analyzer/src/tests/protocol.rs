use crate::protocol::source_name;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use lsp_types::Uri;

#[test]
fn local_file_uris_share_source_keys_with_escaped_windows_paths() {
    let mut db = SourceDatabase::new("C:/project").unwrap();
    let path = "C:\\project\\a b#%中.kgr";
    let id = db.set(path, "base".into(), SourceLayer::Base).unwrap();
    let expected = db.source_name(path).unwrap();
    for text in [
        "file:///C:/project/a%20b%23%25%E4%B8%AD.kgr",
        "FILE://LOCALHOST/c%3A/project/a%20b%23%25%E4%B8%AD.kgr",
        "file:/C:/project/a%20b%23%25%E4%B8%AD.kgr",
    ] {
        let uri: Uri = text.parse().unwrap();
        let key = source_name(&uri).unwrap();
        assert_eq!(key, expected, "{text}");
        assert_eq!(db.file_id(&key), Some(id));
        assert_eq!(
            db.set(&key, "overlay".into(), SourceLayer::Overlay)
                .unwrap(),
            id
        );
    }
    assert_eq!(db.snapshot().files().count(), 1);
    assert_eq!(db.snapshot().file(id).unwrap().text(), "overlay");
}

#[test]
fn file_queries_and_fragments_are_rejected_including_empty_components() {
    for text in [
        "file:///C:/project/main.kgr?version=1",
        "file:///C:/project/main.kgr?",
        "file:///C:/project/main.kgr#section",
        "file:///C:/project/main.kgr#",
        "file:///C:/project/main.kgr?version=1#section",
    ] {
        let uri: Uri = text.parse().unwrap();
        assert_eq!(
            source_name(&uri).unwrap_err(),
            "file document URIs cannot contain a query or fragment",
            "{text}"
        );
    }
}

#[test]
fn file_authorities_exclude_nonlocal_hosts_credentials_and_ports() {
    for text in [
        "file://server/share/main.kgr",
        "file://127.0.0.1/C:/project/main.kgr",
        "file://[::1]/C:/project/main.kgr",
        "file://%6cocalhost/C:/project/main.kgr",
        "file://user:secret@localhost/C:/project/main.kgr",
        "file://@localhost/C:/project/main.kgr",
        "file://localhost:80/C:/project/main.kgr",
        "file://localhost:/C:/project/main.kgr",
        "file://:80/C:/project/main.kgr",
    ] {
        let uri: Uri = text.parse().unwrap();
        assert!(source_name(&uri).is_err(), "{text}");
    }
}

#[test]
fn file_paths_reject_unc_relative_drives_null_and_invalid_utf8() {
    for text in [
        "file:////server/share/main.kgr",
        "file://localhost//server/share/main.kgr",
        "file:///%2Fserver/share/main.kgr",
        "file:///%5Cserver/share/main.kgr",
        "file:///%5C%5Cserver/share/main.kgr",
        "file:%2F%2Fserver/share/main.kgr",
        "file:relative.kgr",
        "file:",
        "file://localhost",
        "file:///C:relative.kgr",
        "file:///C:/project/bad%00.kgr",
        "file:///C:/project/bad%FF.kgr",
    ] {
        let uri: Uri = text.parse().unwrap();
        assert!(source_name(&uri).is_err(), "{text}");
    }
}

#[test]
fn virtual_uris_preserve_scheme_specific_identity_and_only_fold_scheme_case() {
    let mut db = SourceDatabase::new("C:/project").unwrap();
    for (text, expected) in [
        ("UNTITLED:Untitled-1", "untitled:Untitled-1"),
        (
            "Editor+1.-:Case/../Name%2f?Query=Upper#Fragment",
            "editor+1.-:Case/../Name%2f?Query=Upper#Fragment",
        ),
        (
            "MEMORY://Host/../Name%2F?Version=One#Part",
            "memory://Host/../Name%2F?Version=One#Part",
        ),
        ("empty:", "empty:"),
    ] {
        let uri: Uri = text.parse().unwrap();
        let key = source_name(&uri).unwrap();
        assert_eq!(key, expected);
        assert_eq!(db.source_name(&key).unwrap(), expected);
        let id = db
            .set(&key, "virtual".into(), SourceLayer::Overlay)
            .unwrap();
        assert_eq!(db.file_id(text), Some(id));
    }
    let baseline: Uri = "untitled:Untitled-1?View=Upper%2f#Part".parse().unwrap();
    let key = source_name(&baseline).unwrap();
    for text in [
        "untitled:untitled-1?View=Upper%2f#Part",
        "untitled:Untitled-1?View=upper%2f#Part",
        "untitled:Untitled-1?View=Upper%2F#Part",
        "untitled:Untitled-1?View=Upper%2f#part",
        "untitled:Untitled-1?View=Upper%2f",
    ] {
        let other: Uri = text.parse().unwrap();
        assert_ne!(source_name(&other).unwrap(), key, "{text}");
    }
}

#[test]
fn document_uris_require_schemes_and_reject_single_letter_drive_ambiguity() {
    for text in [
        "main.kgr",
        "/project/main.kgr",
        "//server/share/main.kgr",
        "?query",
        "C:/project/main.kgr",
        "c:relative.kgr",
        "x://Host/Name",
        "X:opaque?Query#Fragment",
    ] {
        let uri: Uri = text.parse().unwrap();
        assert!(source_name(&uri).is_err(), "{text}");
    }
}
