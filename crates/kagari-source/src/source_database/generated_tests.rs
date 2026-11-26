use super::*;
use crate::package::PackageScope;
use kagari_common::{identity::PackageId, span::Span};

fn capture(key: &str, display: &str, owner: &str, text: &str) -> CapturedSource {
    CapturedSource {
        key: key.into(),
        display_name: display.into(),
        module: ModuleIdentity::single_file(owner),
        text: text.into(),
    }
}

#[test]
fn generated_refresh_preserves_editor_layers_and_retained_locations() {
    let mut db = SourceDatabase::new("/project").unwrap();
    let cancel = CancellationToken::default();
    let packages =
        PackageEnvironment::new([(PackageId("app".into()), PackageScope::default())].into())
            .unwrap();
    db.replace_captured(
        vec![capture("user.kgr", "user.kgr", "app", "disk")],
        packages.clone(),
        &cancel,
    )
    .unwrap();
    let user = db
        .set("user.kgr", "unsaved".into(), SourceLayer::Overlay)
        .unwrap();
    let key = "mem://host";
    let retained = db
        .replace_generated(
            vec![
                capture(key, "cache/old.kgr", "host", "old"),
                capture("mem://gone", "mem://gone", "gone", "gone"),
            ],
            &cancel,
        )
        .unwrap();
    let id = db.generated_file_id(key).unwrap();
    let old_file = retained.file(id).unwrap();
    let old_span = old_file.span(Span::new(0, 3)).unwrap();

    let refresh = || vec![capture(key, "cache/new.kgr", "host", "updated")];
    let current = db.replace_generated(refresh(), &cancel).unwrap();
    let new_file = current.file(id).unwrap();
    assert_eq!(
        (new_file.name(), new_file.text()),
        ("cache/new.kgr", "updated")
    );
    assert_eq!(current.revision().0, retained.revision().0 + 1);
    assert!(!current.contains(old_span));
    assert!(retained.contains(old_span));
    assert_eq!((old_file.name(), old_file.text()), ("cache/old.kgr", "old"));
    assert_eq!(db.file_id("file:///project/cache/new.kgr"), Some(id));
    assert_eq!(db.file_id("cache/old.kgr"), None);
    assert_eq!(db.file_id("mem://gone"), None);
    assert_eq!(current.files().count(), 2);
    assert_eq!(db.generated_file_id("user.kgr"), None);
    assert!(Arc::ptr_eq(
        current.file(user).unwrap(),
        retained.file(user).unwrap()
    ));
    assert_eq!(current.file(user).unwrap().text(), "unsaved");
    assert_eq!(current.packages(), &packages);
    let same = db.replace_generated(refresh(), &cancel).unwrap();
    assert_eq!(same.revision(), current.revision());
    assert!(Arc::ptr_eq(same.file(id).unwrap(), new_file));
    db.close_overlay("user.kgr").unwrap();
    assert_eq!(db.snapshot().file(user).unwrap().text(), "disk");
    assert_eq!(
        db.snapshot().file(user).unwrap().module_identity(),
        &ModuleIdentity::single_file("app")
    );

    // A whole capture still replaces *all* layers and does not inherit read-only aliases.
    let whole = db.replace_captured(refresh(), packages, &cancel).unwrap();
    assert!(whole.file(user).is_none());
    assert_eq!(db.generated_file_id(key), None);
    assert_eq!(db.file_id("cache/new.kgr"), None);
    assert_eq!(
        db.set(key, "editable".into(), SourceLayer::Overlay)
            .unwrap(),
        id
    );
}

#[test]
fn generated_owner_rejects_alias_edits_and_conflicting_or_cancelled_batches() {
    let mut db = SourceDatabase::new("/project").unwrap();
    let cancel = CancellationToken::default();
    db.replace_captured(
        vec![capture("app://src", "user.kgr", "user", "disk")],
        PackageEnvironment::default(),
        &cancel,
    )
    .unwrap();
    let first = || capture("mem://host", "cache/first.kgr", "host", "first");
    let old = db.replace_generated(vec![first()], &cancel).unwrap();
    let id = db.generated_file_id("mem://host").unwrap();
    for alias in ["MEM://host", "file:///project/cache/%66irst.kgr"] {
        assert_eq!(db.generated_file_id(alias), Some(id));
        for layer in [SourceLayer::Base, SourceLayer::Overlay] {
            assert!(
                db.set(alias, "edit".into(), layer)
                    .unwrap_err()
                    .contains("read-only")
            );
        }
        assert!(
            db.bind_module(alias, ModuleIdentity::single_file("new"))
                .unwrap_err()
                .contains("read-only")
        );
        assert!(db.close_overlay(alias).unwrap_err().contains("read-only"));
    }
    assert!(
        db.bind_module("other", first().module)
            .unwrap_err()
            .contains("already bound")
    );

    let staged = || capture("mem://next", "cache/next.kgr", "next", "next");
    for (key, display, owner, error) in [
        ("MEM://next", "other", "other", "key"),
        ("mem://other", "./cache/next.kgr", "other", "display name"),
        ("cache/next.kgr", "other", "other", "key"),
        ("mem://other", "mem://next", "other", "display name"),
        ("app://src", "other", "other", "key"),
        ("mem://other", "./user.kgr", "other", "display name"),
        ("mem://other", "other", "user", "already bound"),
        ("mem://other", "other", "next", "already bound"),
        ("mem://host", "other", "other", "different module"),
    ] {
        let result = db.replace_generated(
            vec![staged(), capture(key, display, owner, "other")],
            &cancel,
        );
        assert!(result.unwrap_err().contains(error));
    }
    cancel.cancel();
    assert!(
        db.replace_generated(vec![], &cancel)
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(db.file_id("cache/next.kgr"), None);
    assert_eq!(db.file_id("mem://next"), None);
    assert_eq!(db.generated_file_id("cache/first.kgr"), Some(id));
    assert_eq!(db.snapshot().revision(), old.revision());
    assert!(Arc::ptr_eq(
        db.snapshot().file(id).unwrap(),
        old.file(id).unwrap()
    ));
}
