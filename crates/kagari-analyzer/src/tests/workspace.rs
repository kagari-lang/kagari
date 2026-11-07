//! T1 samples real wire FIFO, worker captures and multi-root overlay ownership.
use crate::{
    protocol::source_name,
    server::options::SessionLimits,
    tests::{fixture::Fixture, peer::Peer},
    workspace::{ContextCapture, WorkspaceCapture},
};
use kagari_source::source::SourceFile;
use lsp_types::Uri;
use serde_json::json;
use std::fs;
use tokio::runtime::Builder;

const API: &str = include_str!("../../tests/fixtures/workspace/api.kgr");
const OVERLAY: &str = include_str!("../../tests/fixtures/workspace/api_overlay.kgr");
const ADDED: &str = include_str!("../../tests/fixtures/workspace/added.kgr");
const STANDALONE: &str = include_str!("../../tests/fixtures/workspace/standalone.kgr");
fn context<'a>(capture: &'a WorkspaceCapture, root: &Uri) -> &'a ContextCapture {
    capture
        .contexts
        .iter()
        .find(|context| {
            context
                .origins
                .values()
                .any(|origin| origin.uri.as_ref() == Some(root) && origin.module == context.root)
        })
        .unwrap_or_else(|| panic!("missing root {root:?} in {capture:#?}"))
}

fn source<'a>(context: &'a ContextCapture, uri: &Uri) -> &'a SourceFile {
    let (file, origin) = context
        .origins
        .iter()
        .find(|(_, origin)| origin.uri.as_ref() == Some(uri))
        .unwrap();
    assert!(!origin.read_only);
    assert!(!origin.provider_key.starts_with("source-package:"));
    context.sources.file(*file).unwrap()
}

fn isolated(capture: &WorkspaceCapture, other: &Uri, api: &Uri, added: &Uri) {
    let context = context(capture, other);
    assert!(context.complete && context.semantic_available);
    assert!(
        context
            .origins
            .values()
            .all(|origin| origin.uri.as_ref() != Some(api) && origin.uri.as_ref() != Some(added))
    );
    assert_eq!(
        source(context, other).text(),
        include_str!("../../tests/fixtures/workspace/other.kgr")
    );
}

#[test]
fn wire_ordered_overlays_reach_dependents_and_retained_captures_survive_recapture() {
    let fixture = Fixture::new();
    let first = fixture.uri("first");
    let second = fixture.uri("second");
    let app = fixture.uri("first/app/src/lib.kgr");
    let api = fixture.uri("first/api/src/lib.kgr");
    let added = fixture.uri("first/api/src/added.kgr");
    let other = fixture.uri("second/src/lib.kgr");
    let mut peer = Peer::new(source_name(&first).unwrap(), SessionLimits::default());
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let initialize = peer
            .request(
                1,
                "initialize",
                json!({"capabilities":{},
                    "workspaceFolders":[{"uri":first,"name":"first"},{"uri":second,"name":"second"}]
                }),
            )
            .await;
        assert!(initialize.get("result").is_some(), "{initialize}");
        peer.notify("initialized", json!({})).await;
        let original = peer.capture(1).await;
        assert!(original.errors.is_empty(), "{:?}", original.errors);
        assert_eq!(source(context(&original, &app), &api).text(), API);
        isolated(&original, &other, &api, &added);

        peer.notify(
            "textDocument/didOpen",
            json!({"textDocument":{
                "uri":api,"languageId":"kagari","version":7,"text":OVERLAY
            }}),
        )
        .await;
        peer.notify(
            "textDocument/didOpen",
            json!({"textDocument":{
                "uri":added,"languageId":"kagari","version":1,"text":ADDED
            }}),
        )
        .await;
        let unsaved = peer.capture(3).await;
        assert!(unsaved.errors.is_empty(), "{:?}", unsaved.errors);
        let app_context = context(&unsaved, &app);
        assert!(app_context.complete && app_context.semantic_available);
        assert_eq!(source(app_context, &api).text(), OVERLAY);
        assert_eq!(source(app_context, &added).text(), ADDED);
        let mapping = &unsaved.document_contexts[&source_name(&added).unwrap()];
        assert_eq!(mapping.version, 1);
        let owner = unsaved
            .contexts
            .iter()
            .find(|context| context.key == mapping.context)
            .unwrap();
        assert_eq!(owner.sources.file(mapping.file).unwrap().text(), ADDED);
        assert_eq!(owner.origins[&mapping.file].uri.as_ref(), Some(&added));
        assert!(
            mapping.incarnation
                > unsaved.document_contexts[&source_name(&api).unwrap()].incarnation
        );
        isolated(&unsaved, &other, &api, &added);

        peer.notify(
            "textDocument/didClose",
            json!({"textDocument":{"uri":added}}),
        )
        .await;
        let closed = peer.capture(4).await;
        assert!(!context(&closed, &app).complete);
        assert!(
            context(&closed, &app)
                .errors
                .iter()
                .any(|issue| issue.message.contains("added"))
        );
        assert!(
            !closed
                .document_contexts
                .contains_key(&source_name(&added).unwrap())
        );
        fs::write(fixture.directory().join("first/api/src/added.kgr"), ADDED).unwrap();
        peer.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes":[{"uri":added,"type":1}]}),
        )
        .await;
        let created = peer.capture(5).await;
        assert!(context(&created, &app).complete);
        assert_eq!(source(context(&created, &app), &added).text(), ADDED);
        fs::remove_file(fixture.directory().join("first/api/src/added.kgr")).unwrap();
        peer.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes":[{"uri":added,"type":3}]}),
        )
        .await;
        let deleted = peer.capture(6).await;
        assert!(!context(&deleted, &app).complete);
        assert!(
            context(&deleted, &app)
                .errors
                .iter()
                .any(|issue| issue.message.contains("added"))
        );
        peer.notify("textDocument/didClose", json!({"textDocument":{"uri":api}}))
            .await;
        let restored = peer.capture(7).await;
        assert!(context(&restored, &app).complete);
        assert_eq!(source(context(&restored, &app), &api).text(), API);
        isolated(&restored, &other, &api, &added);

        let scratch: Uri = "custom-editor://scratch/Untitled-1?view=1#body"
            .parse()
            .unwrap();
        peer.notify(
            "textDocument/didOpen",
            json!({"textDocument":{
                "uri":scratch,"languageId":"kagari","version":-3,"text":STANDALONE
            }}),
        )
        .await;
        let standalone = peer.capture(8).await;
        let scratch_context = context(&standalone, &scratch);
        assert!(scratch_context.complete && scratch_context.semantic_available);
        assert_eq!(source(scratch_context, &scratch).text(), STANDALONE);
        assert_eq!(standalone.contexts.len(), restored.contexts.len() + 1);
        assert_eq!(source(context(&original, &app), &api).text(), API);
        assert_eq!(source(context(&unsaved, &app), &api).text(), OVERLAY);
        assert_eq!(source(context(&unsaved, &app), &added).text(), ADDED);
        peer.shutdown().await;
    });
}

#[test]
fn unrepresentable_open_buffer_faults_the_session_instead_of_falling_back_to_disk() {
    let limits = SessionLimits {
        open_documents: 1,
        ..SessionLimits::default()
    };
    let mut peer = Peer::new(env!("CARGO_MANIFEST_DIR").into(), limits);
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let initialized = peer
            .request(1, "initialize", json!({"capabilities":{}}))
            .await;
        assert!(initialized.get("result").is_some());
        peer.notify("initialized", json!({})).await;
        peer.capture(1).await;
        peer.notify(
            "textDocument/didOpen",
            json!({"textDocument":{
                "uri":"untitled:first","languageId":"kagari","version":1,"text":""
            }}),
        )
        .await;
        let admitted = peer.capture(2).await;
        assert_eq!(admitted.document_contexts.len(), 1);
        peer.notify(
            "textDocument/didOpen",
            json!({"textDocument":{
                "uri":"untitled:overflow","languageId":"kagari","version":1,"text":""
            }}),
        )
        .await;
        peer.expect_failure().await;
        assert_eq!(admitted.document_contexts.len(), 1);
    });
}
