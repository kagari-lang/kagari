//! The real worker may finish an old snapshot after a newer edit was admitted.
use crate::{
    server::options::SessionLimits, session::analysis::testing::CompletionGate, tests::peer::Peer,
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::runtime::Builder;

const URI: &str = "untitled:diagnostic-gate";
const ERROR: &str = include_str!("../../tests/fixtures/workspace/standalone_error.kgr");
const CLEAN: &str = include_str!("../../tests/fixtures/workspace/standalone.kgr");

async fn diagnostics(peer: &mut Peer, version: Option<i32>, nonempty: bool) -> Value {
    loop {
        let params = peer.notification("textDocument/publishDiagnostics").await;
        if params["uri"] != URI {
            continue;
        }
        assert_ne!(
            params["version"], 7,
            "held obsolete result reached the writer"
        );
        let actual = params.get("version").and_then(Value::as_i64);
        if actual == version.map(i64::from)
            && params["diagnostics"].as_array().unwrap().is_empty() != nonempty
        {
            return params;
        }
    }
}

#[test]
fn actual_completed_analysis_is_discarded_after_edit_and_current_diagnostics_repair_and_clear() {
    let (gate, held) = CompletionGate::new();
    let mut peer = Peer::with_gate(
        env!("CARGO_MANIFEST_DIR").into(),
        SessionLimits::default(),
        gate,
    );
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let initialize = peer
            .request(
                1,
                "initialize",
                json!({"capabilities":{
                    "textDocument":{"publishDiagnostics":{"versionSupport":true}}
                }}),
            )
            .await;
        assert!(initialize.get("result").is_some());
        peer.notify("initialized", json!({})).await;
        peer.capture(1).await;
        peer.notify(
            "textDocument/didOpen",
            json!({"textDocument":{
                "uri":URI,"languageId":"kagari","version":7,"text":ERROR
            }}),
        )
        .await;
        peer.capture(2).await;
        let completed = held
            .recv_timeout(Duration::from_secs(5))
            .expect("real context projection completion");
        assert_eq!(completed.generation, 2);
        peer.notify(
            "textDocument/didChange",
            json!({"textDocument":{"uri":URI,"version":8},"contentChanges":[{"text":CLEAN}]}),
        )
        .await;
        peer.capture(3).await;
        completed.release.send(()).unwrap();
        peer.handled(completed.job).await;
        assert!(
            diagnostics(&mut peer, Some(8), false).await["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        peer.notify(
            "textDocument/didChange",
            json!({"textDocument":{"uri":URI,"version":9},"contentChanges":[{"text":ERROR}]}),
        )
        .await;
        peer.capture(4).await;
        let errors = diagnostics(&mut peer, Some(9), true).await;
        let mismatch = errors["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|diagnostic| diagnostic["code"] == "KG_TYPE_RETURN_TYPE_MISMATCH")
            .unwrap_or_else(|| panic!("expected semantic type diagnostic: {errors}"));
        assert_eq!(mismatch["severity"], 1);
        assert!(
            mismatch["message"]
                .as_str()
                .is_some_and(|message| message.contains("i32") && message.contains("bool"))
        );
        assert_eq!(
            mismatch["range"],
            json!({"start":{"line":0,"character":0},"end":{"line":2,"character":1}})
        );
        peer.notify("textDocument/didClose", json!({"textDocument":{"uri":URI}}))
            .await;
        peer.capture(5).await;
        assert!(
            diagnostics(&mut peer, None, false).await["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        peer.shutdown().await;
    });
}
