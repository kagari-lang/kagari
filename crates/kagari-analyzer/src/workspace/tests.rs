use crate::{
    documents::DocumentInput,
    protocol::source_name,
    workspace::{
        CaptureFailure, CaptureRequest, MAX_INPUT_BYTES, acquisition, budget::InputBudget,
        origins::file_uri,
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source::SourceFile;
use lsp_types::Uri;
use std::{env, sync::Arc};
use tokio::sync::oneshot;

const SOURCE: &str = include_str!("../../tests/fixtures/benchmark/api.kgr");

fn document(text: &str, version: i32) -> DocumentInput {
    document_at("untitled:WorkspaceTest".parse().unwrap(), text, version)
}

fn document_at(uri: Uri, text: &str, version: i32) -> DocumentInput {
    let source_name = source_name(&uri).unwrap();
    DocumentInput {
        uri,
        version,
        incarnation: 1,
        source: Some(Arc::new(SourceFile::new(&source_name, text))),
        source_name,
        unavailable: false,
    }
}

#[test]
fn retained_input_budget_deduplicates_content_and_releases_only_the_last_lease() {
    let source = Arc::new(SourceFile::new("memory:lease", "x".repeat(128)));
    let bytes = source.retained_bytes();
    let budget = InputBudget::new(bytes);
    let first = budget.retain([source.clone(), source.clone()]).unwrap();
    let second = budget.retain([Arc::new((*source).clone())]).unwrap();
    assert_eq!(budget.retained_bytes(), bytes);
    assert_eq!(first.retained_bytes(), bytes);
    let different = Arc::new(SourceFile::new("memory:another", "different"));
    assert!(budget.retain([source.clone(), different.clone()]).is_err());
    assert_eq!(budget.retained_bytes(), bytes);
    drop(first);
    assert_eq!(budget.retained_bytes(), bytes);
    drop(second);
    assert_eq!(budget.retained_bytes(), 0);
    let admitted = budget.retain([different]).unwrap();
    assert!(budget.retained_bytes() > 0);
    drop(admitted);
    assert_eq!(budget.retained_bytes(), 0);
}

#[test]
fn worker_orders_generations_and_retires_source_epochs_without_losing_retained_views() {
    let budget = InputBudget::new(MAX_INPUT_BYTES);
    let root = env::current_dir().unwrap().to_str().unwrap().to_owned();
    let worker = acquisition::start(root, budget.clone()).unwrap();
    let submit = |generation, documents| {
        let request = CaptureRequest::new(generation, Vec::new(), documents, &budget).unwrap();
        let (sender, receiver) = oneshot::channel();
        worker
            .port
            .submit(request, CancellationToken::default(), sender)
            .unwrap();
        receiver.blocking_recv().unwrap()
    };
    let old = submit(1, vec![document(SOURCE, 1)]).unwrap();
    let old_context = old.contexts[0].clone();
    let old_file = old_context.sources.files().next().unwrap().clone();
    let changed = SOURCE.replace("default_value", "renamed_value");
    let new = submit(2, vec![document(&changed, 2)]).unwrap();
    let new_file = new.contexts[0].sources.files().next().unwrap();
    assert_eq!(old_context.key, new.contexts[0].key);
    assert_eq!(old_file.id(), new_file.id());
    assert!(old_file.revision() < new_file.revision());
    assert_eq!(old_file.text(), SOURCE);
    assert_eq!(new_file.text(), changed);
    assert!(matches!(
        submit(2, Vec::new()),
        Err(CaptureFailure::OutOfOrder {
            previous: 2,
            requested: 2
        })
    ));
    let cancel = CancellationToken::default();
    cancel.cancel();
    let (sender, receiver) = oneshot::channel();
    worker
        .port
        .submit(
            CaptureRequest::new(3, Vec::new(), Vec::new(), &budget).unwrap(),
            cancel,
            sender,
        )
        .unwrap();
    assert!(matches!(
        receiver.blocking_recv().unwrap(),
        Err(CaptureFailure::Cancelled)
    ));
    let empty = submit(4, Vec::new()).unwrap();
    assert!(empty.contexts.is_empty());
    let reopened = submit(5, vec![document(SOURCE, 3)]).unwrap();
    assert_ne!(old_context.key.epoch, reopened.contexts[0].key.epoch);
    assert_ne!(
        old_file.id(),
        reopened.contexts[0].sources.files().next().unwrap().id()
    );
    drop(old);
    drop(new);
    drop(empty);
    drop(reopened);
    drop(worker.port);
    worker.thread.join().unwrap();
    worker.terminated.blocking_recv().unwrap().unwrap();
    assert!(
        budget.retained_bytes() > 0,
        "individually retained context still owns its input lease"
    );
    drop(old_file);
    drop(old_context);
    assert_eq!(budget.retained_bytes(), 0);
}

#[test]
fn file_uri_uses_utf8_escapes_and_rejects_semantic_or_relative_paths() {
    assert_eq!(
        file_uri("C:/project/a b#%中.kgr").unwrap().as_str(),
        "file:///c:/project/a%20b%23%25%E4%B8%AD.kgr"
    );
    assert_eq!(
        file_uri("/project/a b.kgr").unwrap().as_str(),
        "file:///project/a%20b.kgr"
    );
    assert_eq!(
        file_uri(r"\\?\C:\project\a b.kgr").unwrap().as_str(),
        "file:///c:/project/a%20b.kgr"
    );
    for invalid in [
        "relative.kgr",
        "package://semantic/unit",
        "//remote/share.kgr",
        "C:relative.kgr",
    ] {
        assert!(file_uri(invalid).is_err(), "{invalid}");
    }
}
mod benchmark;
mod boundaries;
