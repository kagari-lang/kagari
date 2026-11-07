//! Cancellation retires a real index continuation without another client request.
use crate::{
    server::options::SessionLimits,
    session::analysis::testing::CompletionGate,
    tests::{
        features::{API, site},
        peer::Peer,
    },
};
use serde_json::json;
use std::time::Duration;
use tokio::runtime::Builder;

#[test]
fn cancelled_index_cursor_retires_while_client_is_idle() {
    const URI: &str = "untitled:idle-index-cancellation";
    let (gate, continued) = CompletionGate::new();
    let mut peer = Peer::with_step_gate(
        env!("CARGO_MANIFEST_DIR").into(),
        SessionLimits::default(),
        gate,
    );
    Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let initialize = peer
                .request(
                    1,
                    "initialize",
                    json!({"capabilities":{
                        "textDocument":{"publishDiagnostics":{"versionSupport":true}}
                    }}),
                )
                .await;
            assert_eq!(
                initialize["result"]["capabilities"]["implementationProvider"]["workDoneProgress"],
                true
            );
            peer.notify("initialized", json!({})).await;
            peer.notify(
                "textDocument/didOpen",
                json!({"textDocument":{
                    "uri":URI, "languageId":"kagari", "version":1, "text":API
                }}),
            )
            .await;
            peer.capture(2).await;
            // This is the only context. Its versioned publication comes from actual
            // completed analysis, so no background job can incidentally prune later.
            loop {
                let completed = peer.notification("textDocument/publishDiagnostics").await;
                if completed["uri"] == URI && completed["version"] == 1 {
                    assert!(completed["diagnostics"].as_array().unwrap().is_empty());
                    break;
                }
            }
            peer.begin_request(2, "textDocument/implementation", site(URI, API, "Read {"))
                .await;
            let held = continued
                .recv_timeout(Duration::from_secs(5))
                .expect("actual index Continue");
            assert_eq!(held.generation, 2);
            peer.notify("$/cancelRequest", json!({"id":2})).await;
            assert_eq!(peer.response(2).await["error"]["code"], -32800);
            held.release.send(()).unwrap();
            // No query, edit or shutdown is sent until the independent maintenance
            // completion proves that the cancelled request no longer owns a cursor.
            assert_eq!(peer.maintained_after(held.job).await, 0);
            peer.shutdown().await;
        });
}
