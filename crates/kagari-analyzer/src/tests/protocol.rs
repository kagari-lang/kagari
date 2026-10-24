use crate::protocol::source_name;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use lsp_types::Uri;
use rstest::rstest;

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

#[rstest]
#[case::query("file:///C:/project/main.kgr?version=1")]
#[case::empty_query("file:///C:/project/main.kgr?")]
#[case::fragment("file:///C:/project/main.kgr#section")]
#[case::empty_fragment("file:///C:/project/main.kgr#")]
#[case::query_and_fragment("file:///C:/project/main.kgr?version=1#section")]
fn file_queries_and_fragments_are_rejected_including_empty_components(#[case] text: &str) {
    let uri: Uri = text.parse().unwrap();
    assert_eq!(
        source_name(&uri).unwrap_err(),
        "file document URIs cannot contain a query or fragment",
        "{text}"
    );
}

#[rstest]
#[case::remote_host("file://server/share/main.kgr")]
#[case::ipv4_loopback("file://127.0.0.1/C:/project/main.kgr")]
#[case::ipv6_loopback("file://[::1]/C:/project/main.kgr")]
#[case::encoded_localhost("file://%6cocalhost/C:/project/main.kgr")]
#[case::credentials("file://user:secret@localhost/C:/project/main.kgr")]
#[case::empty_userinfo("file://@localhost/C:/project/main.kgr")]
#[case::port("file://localhost:80/C:/project/main.kgr")]
#[case::empty_port("file://localhost:/C:/project/main.kgr")]
#[case::empty_host_port("file://:80/C:/project/main.kgr")]
fn file_authorities_exclude_nonlocal_hosts_credentials_and_ports(#[case] text: &str) {
    let uri: Uri = text.parse().unwrap();
    assert!(source_name(&uri).is_err(), "{text}");
}

#[rstest]
#[case::unc_slashes("file:////server/share/main.kgr")]
#[case::localhost_unc("file://localhost//server/share/main.kgr")]
#[case::encoded_slash("file:///%2Fserver/share/main.kgr")]
#[case::encoded_backslash("file:///%5Cserver/share/main.kgr")]
#[case::encoded_double_backslash("file:///%5C%5Cserver/share/main.kgr")]
#[case::encoded_unc_prefix("file:%2F%2Fserver/share/main.kgr")]
#[case::relative("file:relative.kgr")]
#[case::empty("file:")]
#[case::authority_without_path("file://localhost")]
#[case::relative_drive("file:///C:relative.kgr")]
#[case::nul("file:///C:/project/bad%00.kgr")]
#[case::invalid_utf8("file:///C:/project/bad%FF.kgr")]
fn file_paths_reject_unc_relative_drives_null_and_invalid_utf8(#[case] text: &str) {
    let uri: Uri = text.parse().unwrap();
    assert!(source_name(&uri).is_err(), "{text}");
}

#[rstest]
#[case::untitled("UNTITLED:Untitled-1", "untitled:Untitled-1")]
#[case::extended_scheme(
    "Editor+1.-:Case/../Name%2f?Query=Upper#Fragment",
    "editor+1.-:Case/../Name%2f?Query=Upper#Fragment"
)]
#[case::memory_authority(
    "MEMORY://Host/../Name%2F?Version=One#Part",
    "memory://Host/../Name%2F?Version=One#Part"
)]
#[case::empty_opaque("empty:", "empty:")]
fn virtual_uris_preserve_scheme_specific_identity_and_only_fold_scheme_case(
    #[case] text: &str,
    #[case] expected: &str,
) {
    let mut db = SourceDatabase::new("C:/project").unwrap();
    let uri: Uri = text.parse().unwrap();
    let key = source_name(&uri).unwrap();
    assert_eq!(key, expected);
    assert_eq!(db.source_name(&key).unwrap(), expected);
    let id = db
        .set(&key, "virtual".into(), SourceLayer::Overlay)
        .unwrap();
    assert_eq!(db.file_id(text), Some(id));
}

#[test]
fn virtual_uri_near_matches_remain_distinct() {
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

#[rstest]
#[case::relative_file("main.kgr")]
#[case::absolute_path_without_scheme("/project/main.kgr")]
#[case::unc_without_scheme("//server/share/main.kgr")]
#[case::query_without_scheme("?query")]
#[case::drive_path("C:/project/main.kgr")]
#[case::relative_drive_path("c:relative.kgr")]
#[case::single_letter_hierarchical("x://Host/Name")]
#[case::single_letter_opaque("X:opaque?Query#Fragment")]
fn document_uris_require_schemes_and_reject_single_letter_drive_ambiguity(#[case] text: &str) {
    let uri: Uri = text.parse().unwrap();
    assert!(source_name(&uri).is_err(), "{text}");
}
