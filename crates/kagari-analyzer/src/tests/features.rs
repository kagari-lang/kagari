//! Real actor requests retain their admitted overlays while later edits proceed.
use crate::{
    protocol::source_name,
    server::options::SessionLimits,
    session::analysis::testing::CompletionGate,
    tests::{fixture::Fixture, peer::Peer},
    workspace::origins::file_uri,
};
use serde_json::{Value, json, to_vec_pretty};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    time::{Duration, Instant},
};
use tokio::runtime::Builder;

const LIB: &str = include_str!("../../tests/fixtures/editor/src/lib.kgr");
const API: &str = include_str!("../../tests/fixtures/editor/src/api.kgr");

#[cfg(feature = "verification")]
mod cancellation;
mod maintenance;

fn position(text: &str, needle: &str) -> Value {
    let offset = text.find(needle).expect("fixed fixture site");
    let prefix = &text[..offset];
    json!({"line":prefix.bytes().filter(|byte| *byte == b'\n').count(),"character":prefix.rsplit('\n').next().unwrap().encode_utf16().count()})
}

fn site(uri: &str, text: &str, needle: &str) -> Value {
    json!({"textDocument":{"uri":uri},"position":position(text,needle)})
}

fn hover_contains(response: &Value, text: &str) {
    assert!(response.get("error").is_none(), "{response}");
    assert!(
        response["result"]["contents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|content| {
                content
                    .get("value")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.contains(text))
            }),
        "expected hover {text}: {response}"
    );
}

#[test]
fn pilot_features_preserve_request_inputs_and_cancel_without_waiting_for_worker_credit() {
    let fixture = Fixture::new();
    let project = fixture.directory().join("editor # 空格");
    for file in ["Kagari.toml", "src/lib.kgr", "src/api.kgr"] {
        let destination = project.join(file);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/editor")
                .join(file),
            destination,
        )
        .unwrap();
    }
    let root = file_uri(project.to_str().unwrap()).unwrap();
    let unrelated = fixture.uri("second");
    fs::write(
        fixture.directory().join("second/src/lib.kgr"),
        format!(
            "mod missing;\n{}",
            include_str!("../../tests/fixtures/workspace/other.kgr")
        ),
    )
    .unwrap();
    let lib = file_uri(project.join("src/lib.kgr").to_str().unwrap()).unwrap();
    let alias = project.join("open api alias 雪 #");
    fixture.link_directory(&project.join("src"), &alias);
    let api = file_uri(alias.join("api.kgr").to_str().unwrap()).unwrap();
    let overlay = API.replace(
        "pub fn value()",
        "// Unsaved navigation line.\npub fn value()",
    );
    let (gate, held) = CompletionGate::new();
    let mut peer = Peer::with_gate(source_name(&root).unwrap(), SessionLimits::default(), gate);
    Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        // Lifecycle errors win even when method-specific decoding already failed.
        let before = peer.request(0, "textDocument/hover", json!({})).await;
        assert_eq!(before["error"]["code"], -32002);
        let initialized = peer.request(1, "initialize", json!({"capabilities":{"window":{"workDoneProgress":true}},"workspaceFolders":[{"uri":root.as_str(),"name":"editor"},{"uri":unrelated.as_str(),"name":"unrelated broken project"}]})).await;
        assert_eq!(initialized["result"]["capabilities"]["hoverProvider"],true);
        assert_eq!(initialized["result"]["capabilities"]["implementationProvider"]["workDoneProgress"],true);
        peer.notify("initialized",json!({})).await;
        for (uri,text) in [(&lib,LIB),(&api,overlay.as_str())] {
            peer.notify("textDocument/didOpen",json!({"textDocument":{"uri":uri.as_str(),"languageId":"kagari","version":1,"text":text}})).await;
        }
        peer.capture(3).await;
        let completed = held.recv_timeout(Duration::from_secs(5)).expect("actual diagnostic worker held outside actor");
        // A valid document position cannot hide an invalid completion-specific field.
        // The real worker is held, so this must reject during admission rather than queue.
        let mut malformed = site(lib.as_str(), LIB, "answer\n");
        malformed["context"] = json!({"triggerKind":"invalid"});
        let rejected = peer.request(10, "textDocument/completion", malformed).await;
        assert_eq!(rejected["error"]["code"], -32602);
        let mut oversized = site(lib.as_str(), LIB, "answer\n");
        oversized["context"] = json!({"triggerKind":2,"triggerCharacter":"x".repeat(64*1024)});
        assert_eq!(peer.request(11, "textDocument/completion", oversized).await["error"]["code"], -32602);
        let boolean = overlay.replace("pub fn value() -> i32 {\n    42", "pub fn value() -> bool {\n    true");
        let wide = overlay.replace("pub fn value() -> i32 {\n    42", "pub fn value() -> i64 {\n    42");
        peer.notify("textDocument/didChange",json!({"textDocument":{"uri":api.as_str(),"version":2},"contentChanges":[{"text":boolean}]})).await;
        peer.begin_request(20,"textDocument/hover",site(lib.as_str(),LIB,"answer\n")).await;
        peer.notify("textDocument/didChange",json!({"textDocument":{"uri":api.as_str(),"version":3},"contentChanges":[{"text":wide}]})).await;
        let mut cancelled_index = site(api.as_str(),API,"Read {");
        cancelled_index["workDoneToken"] = json!("cancel-index");
        peer.begin_request(21,"textDocument/implementation",cancelled_index).await;
        let cancel_started = Instant::now();
        peer.notify("window/workDoneProgress/cancel",json!({"token":"cancel-index"})).await;
        assert_eq!(peer.response(21).await["error"]["code"],-32800);
        let queued_cancel_ms = cancel_started.elapsed().as_secs_f64() * 1000.0;
        let report_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/lsp-benchmark");
        fs::create_dir_all(&report_directory).unwrap();
        fs::write(report_directory.join("B06-queued-cancel.json"), to_vec_pretty(&json!({
            "method":"textDocument/implementation", "client_cancel_to_terminal_ms":queued_cancel_ms,
            "cancel_method":"window/workDoneProgress/cancel",
            "target_ms":50, "target_met_for_sample":queued_cancel_ms <= 50.0,
            "scope":"real actor; queued implementation cancelled while prior real diagnostic worker completion is held; one correctness sample, not p95",
            "prior_worker_job":completed.job,"prior_worker_generation":completed.generation
        })).unwrap()).unwrap();
        peer.capture(5).await;
        completed.release.send(()).unwrap();
        hover_contains(&peer.response(20).await,"answer: bool");
        hover_contains(&peer.request(21,"textDocument/hover",site(lib.as_str(),LIB,"answer\n")).await,"answer: i64");
        let definition = peer.request(22,"textDocument/definition",site(lib.as_str(),LIB,"value();")).await;
        assert_eq!(definition["result"]["uri"],api.as_str(),"{definition}");
        assert_eq!(definition["result"]["range"]["start"],position(&wide,"value()"));
        assert_ne!(position(&wide,"value()"),position(API,"value()"));
        assert_eq!(fs::read_to_string(project.join("src/api.kgr")).unwrap(),API);
        let completion = peer.request(23,"textDocument/completion",site(lib.as_str(),LIB,"answer\n")).await;
        let item = completion["result"]["items"].as_array().unwrap().iter().find(|item|item["label"]=="answer").expect("visible typed lexical binding");
        assert_eq!(item["detail"],"i64");
        assert_eq!(item["textEdit"]["newText"],"answer");
        assert_eq!(item["textEdit"]["range"],json!({"start":{"line":6,"character":4},"end":{"line":6,"character":10}}));
        let mut indexed_site = site(api.as_str(),API,"Read {");
        indexed_site["workDoneToken"] = json!("complete-index");
        let implementation = peer.request(24,"textDocument/implementation",indexed_site).await;
        let mut progress = BTreeMap::<String, Vec<String>>::new();
        while progress.get("complete-index").is_none_or(|events| events.last().map(String::as_str) != Some("end")) {
            let notification = peer.notification("$/progress").await;
            progress.entry(notification["token"].as_str().unwrap().to_owned()).or_default().push(notification["value"]["kind"].as_str().unwrap().to_owned());
        }
        assert_eq!(progress["cancel-index"], ["begin", "end"]);
        let completed_progress = &progress["complete-index"];
        assert_eq!(completed_progress.first().unwrap(), "begin");
        assert!(completed_progress[1..completed_progress.len()-1].iter().all(|event| event == "report"));
        let destinations = implementation["result"].as_array().unwrap_or_else(||panic!("{implementation}"));
        assert_eq!(destinations.len(),1,"{implementation}");
        assert_eq!(destinations[0]["uri"],api.as_str());
        assert_eq!(destinations[0]["range"]["start"],position(API,"Reading {\n    fn"));
        let native = peer.request(25,"textDocument/definition",site(lib.as_str(),LIB,"create();")).await;
        let uri = native["result"]["uri"].as_str().unwrap_or_else(||panic!("{native}"));
        let native_uri = uri.parse().unwrap();
        let text = fs::read_to_string(source_name(&native_uri).unwrap()).expect("validated native destination is published before queries run");
        assert!(text.contains("fn new"));
        assert_ne!(uri,lib.as_str());
        peer.shutdown().await;
    });
}
