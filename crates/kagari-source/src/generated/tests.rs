use super::*;
use std::{sync::Barrier, thread};

const SOURCE: &str = include_str!("../../tests/fixtures/generated/original.kgr");
const CHANGED: &str = include_str!("../../tests/fixtures/generated/changed.kgr");
static NEXT: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/generated-document-tests/中文 cache")
        .join(format!(
            "{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&directory).unwrap();
    fs::canonicalize(directory).unwrap()
}

#[test]
fn planned_documents_publish_concurrently_and_preserve_existing_content() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<GeneratedDocuments>();
    send_sync::<GeneratedDocument>();
    let root = scratch();
    let store = GeneratedDocuments::open(&root, "renderer-v1").unwrap();
    let text: Arc<str> = SOURCE.into();
    let original = store.plan(text.clone());
    let identical = store.plan(text);
    let changed = store.plan(CHANGED.into());
    let next_renderer = GeneratedDocuments::open(&root, "renderer-v2")
        .unwrap()
        .plan(SOURCE.into());
    assert_eq!(original.path(), identical.path());
    assert_eq!(original.source_name(), identical.source_name());
    assert_ne!(original.path(), changed.path());
    assert_ne!(original.path(), next_renderer.path());
    assert!(!original.path().exists());
    assert!(!changed.path().exists());
    assert_eq!(
        source_name(original.path()).unwrap(),
        original.source_name()
    );
    let barrier = Barrier::new(4);
    thread::scope(|scope| {
        for _ in 0..4 {
            let document = &original;
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                document.publish().unwrap();
            });
        }
    });
    changed.publish().unwrap();
    assert_eq!(fs::read_to_string(original.path()).unwrap(), SOURCE);
    assert_eq!(fs::read_to_string(changed.path()).unwrap(), CHANGED);
    assert_eq!(
        fs::read_dir(original.path().parent().unwrap())
            .unwrap()
            .count(),
        2,
        "publication must clean up its temporary files"
    );
    // Same-length corruption must be compared, not accepted from metadata alone.
    let corrupt = vec![b'!'; SOURCE.len()];
    fs::write(original.path(), &corrupt).unwrap();
    let error = identical.publish().unwrap_err();
    assert_eq!(error.path, original.path());
    assert_eq!(error.error.kind(), ErrorKind::InvalidData);
    assert_eq!(fs::read(original.path()).unwrap(), corrupt);
    assert_eq!(original.text(), SOURCE);
    assert_eq!(fs::read_to_string(changed.path()).unwrap(), CHANGED);
}

#[test]
fn store_and_publication_failures_retain_paths_without_creating_a_target() {
    let root = scratch();
    let blocked = root.join("file");
    fs::write(&blocked, SOURCE).unwrap();
    let error = GeneratedDocuments::open(&blocked, "renderer-v1").unwrap_err();
    assert_eq!(error.path, blocked.join("renderer-v1"));
    assert_ne!(error.error.kind(), ErrorKind::NotFound);
    assert_eq!(fs::read_to_string(&blocked).unwrap(), SOURCE);
    assert_eq!(
        GeneratedDocuments::open(&root, "../escape")
            .unwrap_err()
            .error
            .kind(),
        ErrorKind::InvalidInput
    );
    let store = GeneratedDocuments::open(&root, "removed").unwrap();
    let document = store.plan(SOURCE.into());
    fs::remove_dir(document.path().parent().unwrap()).unwrap();
    // Planning remains pure after opening, even if the directory disappears.
    assert_eq!(
        store.plan(SOURCE.into()).source_name(),
        document.source_name()
    );
    let error = document.publish().unwrap_err();
    assert_eq!(error.path.parent(), document.path().parent());
    assert_eq!(error.error.kind(), ErrorKind::NotFound);
    assert!(!document.path().exists());
}
