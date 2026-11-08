use super::*;
use std::{sync::Barrier, thread};

const SOURCE: &str = include_str!("../../tests/fixtures/generated/original.kgr");
const CHANGED: &str = include_str!("../../tests/fixtures/generated/changed.kgr");
static NEXT: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/source-publication-tests/中文 cache")
        .join(format!(
            "{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&directory).unwrap();
    fs::canonicalize(directory).unwrap()
}

fn file(path: &str, text: &str) -> PublicationSource {
    PublicationSource {
        path: path.into(),
        text: text.into(),
    }
}

#[test]
fn packages_publish_readable_paths_concurrently_and_keep_exact_old_snapshots() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<SourceStore>();
    send_sync::<SourceDocument>();
    let root = scratch().join("cache-not-created");
    let store = SourceStore::open(&root).unwrap();
    let original = store
        .plan(
            "stdlib",
            vec![
                file("core/lib.kgr", SOURCE),
                file("core/other.kgr", CHANGED),
            ],
        )
        .unwrap();
    let reordered = store
        .plan(
            "stdlib",
            vec![
                file("core/other.kgr", CHANGED),
                file("core/lib.kgr", SOURCE),
            ],
        )
        .unwrap();
    assert_eq!(original[0].path(), reordered[1].path());
    assert!(original[0].path().ends_with("core/lib.kgr"));
    assert!(
        !root.exists(),
        "source planning must not require a writable cache"
    );
    let changed = store
        .plan(
            "stdlib",
            vec![file("core/lib.kgr", SOURCE), file("core/other.kgr", SOURCE)],
        )
        .unwrap();
    assert_ne!(
        original[0].path(),
        changed[0].path(),
        "the whole source set owns one immutable identity"
    );
    let barrier = Barrier::new(4);
    thread::scope(|scope| {
        for _ in 0..4 {
            let barrier = &barrier;
            let original = &original;
            scope.spawn(move || {
                barrier.wait();
                for document in original {
                    document.publish().unwrap();
                }
            });
        }
    });
    for document in &changed {
        document.publish().unwrap();
    }
    assert_eq!(fs::read_to_string(original[1].path()).unwrap(), CHANGED);
    assert_eq!(fs::read_to_string(changed[1].path()).unwrap(), SOURCE);
    let corrupt = vec![b'!'; SOURCE.len()];
    fs::write(original[0].path(), &corrupt).unwrap();
    let error = original[0].publish().unwrap_err();
    assert_eq!(error.path, original[0].path());
    assert_eq!(error.error.kind(), ErrorKind::InvalidData);
    assert_eq!(fs::read(original[0].path()).unwrap(), corrupt);
    assert_eq!(fs::read_to_string(changed[0].path()).unwrap(), SOURCE);
    assert_eq!(
        fs::read_dir(original[0].path().parent().unwrap())
            .unwrap()
            .count(),
        2,
        "temporary files must be retired"
    );
}

#[test]
fn package_paths_reject_traversal_platform_aliases_and_collisions() {
    let store = SourceStore::open(&scratch()).unwrap();
    for path in [
        "../escape.kgr",
        "/absolute.kgr",
        "core\\escape.kgr",
        "core/../escape.kgr",
        "core/con.kgr",
    ] {
        assert_eq!(
            store
                .plan("stdlib", vec![file(path, SOURCE)])
                .unwrap_err()
                .error
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    assert!(
        store
            .plan("../outside", vec![file("lib.kgr", SOURCE)])
            .is_err()
    );
    assert!(
        store
            .plan(
                "host",
                vec![file("same.kgr", SOURCE), file("Same.kgr", CHANGED)]
            )
            .is_err()
    );
    let lower = module_path("application", &["api".into()]);
    let upper = module_path("Application", &["api".into()]);
    assert_ne!(lower.to_lowercase(), upper.to_lowercase());
    for path in [
        lower,
        upper,
        module_path("con", &["aux".into()]),
        module_path("雪", &["a/b".into()]),
    ] {
        assert!(store.plan("host", vec![file(&path, SOURCE)]).is_ok());
    }
}

#[test]
fn blocked_publication_preserves_semantic_text_and_missing_files_are_restored() {
    let root = scratch();
    let blocked = root.join("blocked");
    fs::write(&blocked, SOURCE).unwrap();
    let store = SourceStore::open(&blocked).unwrap();
    let document = store
        .plan("stdlib", vec![file("core/lib.kgr", SOURCE)])
        .unwrap()
        .remove(0);
    let error = document.publish().unwrap_err();
    assert_eq!(error.path, blocked);
    assert_eq!(document.text(), SOURCE);
    assert_eq!(fs::read_to_string(blocked).unwrap(), SOURCE);
    let store = SourceStore::open(&root.join("restored")).unwrap();
    let document = store
        .plan("stdlib", vec![file("core/lib.kgr", SOURCE)])
        .unwrap()
        .remove(0);
    document.publish().unwrap();
    fs::remove_file(document.path()).unwrap();
    document.publish().unwrap();
    assert_eq!(fs::read_to_string(document.path()).unwrap(), SOURCE);
}

#[test]
fn installed_sources_are_checked_read_only_and_mismatched_sets_use_the_cache() {
    let root = scratch();
    let installed = root.join("installed");
    fs::create_dir(&installed).unwrap();
    fs::write(installed.join("lib.kgr"), SOURCE).unwrap();
    let cache = root.join("cache-file");
    fs::write(&cache, "not a directory").unwrap();
    let mut store = SourceStore::open(&cache).unwrap();
    let sources = vec![file("lib.kgr", SOURCE)];
    store
        .prefer_installed("stdlib", &installed, &sources)
        .unwrap();
    let original = store.plan("stdlib", sources).unwrap().remove(0);
    assert_eq!(original.path(), installed.join("lib.kgr"));
    original.publish().unwrap();
    let changed = store
        .plan("stdlib", vec![file("lib.kgr", CHANGED)])
        .unwrap()
        .remove(0);
    assert_ne!(changed.path(), original.path());
    assert!(changed.publish().is_err());
    fs::write(original.path(), CHANGED).unwrap();
    assert_eq!(
        original.publish().unwrap_err().error.kind(),
        ErrorKind::InvalidData
    );
    assert!(
        store
            .prefer_installed("stdlib", &installed, &[file("lib.kgr", SOURCE)])
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(original.path()).unwrap(),
        CHANGED,
        "installed sources must never be overwritten"
    );
}
