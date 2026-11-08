use crate::{
    documents::DocumentInput,
    protocol::source_name,
    session::request_tasks::{RequestTask, RequestTasks},
    workspace::{CaptureRequest, WorkspaceCapture, acquisition, budget::InputBudget},
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source::SourceFile;
use lsp_types::Uri;
use std::sync::Arc;
use tokio::sync::oneshot;

fn task(serial: u64, name: &str, capture: &Arc<WorkspaceCapture>) -> RequestTask {
    RequestTask {
        serial,
        generation: capture.generation,
        source_name: name.into(),
        capture: Some(capture.clone()),
        dispatch: Box::new(|_, _, _| Ok(())),
    }
}

#[test]
fn index_continuation_yields_to_another_contexts_already_queued_query() {
    let budget = InputBudget::new(4 * 1024 * 1024);
    let worker = acquisition::start(
        env!("CARGO_MANIFEST_DIR").replace('\\', "/"),
        budget.clone(),
    )
    .unwrap();
    let documents: Vec<_> = ["untitled:scheduler-a", "untitled:scheduler-b"]
        .into_iter()
        .map(|name| {
            let uri: Uri = name.parse().unwrap();
            let name = source_name(&uri).unwrap();
            DocumentInput {
                source: Some(Arc::new(SourceFile::new(
                    &name,
                    include_str!("../../../tests/fixtures/workspace/standalone.kgr"),
                ))),
                source_name: name,
                uri,
                version: 1,
                incarnation: 1,
                unavailable: false,
            }
        })
        .collect();
    let (sender, receiver) = oneshot::channel();
    worker
        .port
        .submit(
            CaptureRequest::new(1, vec![], documents.clone(), &budget).unwrap(),
            CancellationToken::default(),
            sender,
        )
        .unwrap();
    let capture = Arc::new(receiver.blocking_recv().unwrap().unwrap());
    assert_eq!(capture.contexts.len(), 2);
    let a = &documents[0].source_name;
    let b = &documents[1].source_name;
    assert_ne!(
        capture.document_contexts[a].context,
        capture.document_contexts[b].context
    );
    let mut scheduler = RequestTasks::default();
    scheduler.pending.extend([
        task(1, a, &capture),
        task(2, a, &capture),
        task(3, b, &capture),
    ]);
    assert_eq!(scheduler.next_ready().unwrap().unwrap().serial, 1);
    assert_eq!(scheduler.next_ready().unwrap().unwrap().serial, 3);
    // B's first index chunk returns to the same scheduler as ordinary work.
    // A's second request must run before B acquires another worker turn.
    scheduler.pending.push_back(task(4, b, &capture));
    assert_eq!(scheduler.next_ready().unwrap().unwrap().serial, 2);
    assert_eq!(scheduler.next_ready().unwrap().unwrap().serial, 4);
    assert!(scheduler.next_ready().unwrap().is_none());
    drop(worker.port);
    worker.thread.join().unwrap();
}
