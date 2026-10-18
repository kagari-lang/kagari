//! Adapter contracts are portable; browser linkage/execution is owned by the harness.
use crate::BrowserSession;
#[cfg(feature = "source")]
use crate::output::{self, Output};
#[cfg(feature = "source")]
use kagari_source::source::SourceFile;
use serde_json::{Value, json};
#[cfg(feature = "source")]
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

fn response(text: String) -> Value {
    serde_json::from_str(&text).unwrap()
}

#[test]
fn artifact_validation_and_idle_state_are_real_without_source() {
    let mut session = BrowserSession::new().unwrap();
    assert_eq!(response(session.start())["error"]["code"], "WEB_NO_PROGRAM");
    assert_eq!(
        response(session.drive(0))["error"]["code"],
        "WEB_SLICE_LIMIT"
    );
    assert_eq!(
        response(session.drive(100_001))["error"]["code"],
        "WEB_SLICE_LIMIT"
    );
    assert_eq!(
        response(session.load_artifact(b"not a checked artifact"))["status"],
        "error"
    );
    assert_eq!(response(session.reset())["status"], "idle");
    session.cancel();
    assert_eq!(response(session.drain_output())["output"], json!([]));
}

#[cfg(feature = "source")]
fn compile(session: &mut BrowserSession, source: &str) {
    let result = response(session.compile(source));
    assert_eq!(result["status"], "ready", "{result}");
}

#[cfg(feature = "source")]
fn finish(session: &mut BrowserSession) -> Value {
    let roots = session.runtime.runtime().gc().active_roots();
    assert_eq!(response(session.start())["status"], "runnable");
    for _ in 0..100 {
        let result = response(session.drive(1000));
        if result["status"] != "runnable" {
            assert_eq!(session.runtime.runtime().gc().active_roots(), roots);
            session.runtime.runtime().collect_garbage().unwrap();
            return result;
        }
    }
    panic!("bounded fixture did not complete");
}

#[cfg(feature = "source")]
#[test]
fn main_output_and_rooted_results_cross_the_owned_boundary() {
    let mut session = BrowserSession::new().unwrap();
    compile(
        &mut session,
        "use web::print; fn main() -> u64 { print(\"hello 😀\"); 18446744073709551615u64 }",
    );
    let result = finish(&mut session);
    assert_eq!(result["status"], "complete", "{result}");
    assert_eq!(result["output"], json!(["hello 😀"]));
    assert_eq!(
        result["result"],
        json!({"tag":"u64", "value":"18446744073709551615"})
    );
    assert_eq!(response(session.drain_output())["output"], json!([]));
    compile(&mut session, "fn main() -> String { \"rooted 😀\" }");
    assert_eq!(
        finish(&mut session)["result"],
        json!({"tag":"string", "value":"rooted 😀", "truncated":false})
    );
    compile(&mut session, "fn main() -> (i32, i32) { (1, 2) }");
    assert_eq!(
        finish(&mut session)["result"],
        json!({"tag":"opaque", "kind":"tuple"})
    );
}

#[cfg(feature = "source")]
#[test]
fn source_diagnostics_retain_origin_and_utf16_ranges() {
    let mut session = BrowserSession::new().unwrap();
    let result = response(session.compile("// 😀\r\nfn main() -> i32 { missing }"));
    assert_eq!(result["status"], "error");
    let diagnostic = &result["diagnostics"][0];
    assert_eq!(diagnostic["severity"], "error");
    assert!(diagnostic["code"].as_str().unwrap().starts_with("KG_"));
    let location = &diagnostic["location"];
    assert_eq!(location["origin"], "memory://main.kgr");
    assert_eq!(location["start"]["line"], 1);
    assert_eq!(location["start"]["character"], 19);
    assert_eq!(location["byte_start"], 28);
    assert_eq!(location["start"]["offset"], 26);
    let too_large = " ".repeat(super::SOURCE_BYTES + 1);
    assert_eq!(
        response(session.compile(&too_large))["error"]["code"],
        "WEB_SOURCE_LIMIT"
    );
}

#[cfg(feature = "source")]
#[test]
fn exclusive_run_cancellation_reset_and_failed_candidates_preserve_state() {
    let mut session = BrowserSession::new().unwrap();
    compile(&mut session, "fn main() { loop {} }");
    let roots = session.runtime.runtime().gc().active_roots();
    assert_eq!(response(session.start())["status"], "runnable");
    assert_eq!(response(session.drive(10))["status"], "runnable");
    assert_eq!(response(session.start())["error"]["code"], "WEB_BUSY");
    assert_eq!(
        response(session.compile("fn main() {}"))["error"]["code"],
        "WEB_BUSY"
    );
    assert_eq!(
        response(session.load_artifact(&[]))["error"]["code"],
        "WEB_BUSY"
    );
    session.cancel();
    let result = response(session.drive(100));
    assert_eq!(result["error"]["code"], "KG_RUNTIME_CANCELLED", "{result}");
    assert!(session.active.is_none());
    assert_eq!(session.runtime.runtime().gc().active_roots(), roots);
    assert_eq!(response(session.start())["status"], "runnable");
    assert_eq!(response(session.reset())["status"], "idle");
    assert_eq!(response(session.start())["error"]["code"], "WEB_NO_PROGRAM");
    compile(&mut session, "fn main() -> i32 { 42 }");
    assert_eq!(response(session.load_artifact(&[]))["status"], "error");
    assert_eq!(response(session.compile("not source"))["status"], "error");
    assert_eq!(finish(&mut session)["result"]["value"], "42");
}

#[cfg(feature = "source")]
#[test]
fn generate_browser_artifact() {
    let engine = output::engine(Arc::new(Mutex::new(Output::default()))).unwrap();
    let artifact = engine.compile_to_artifact(
        SourceFile::new("memory://browser-artifact.kgr", "use web::print; fn main() -> u64 { print(\"artifact hello 😀\"); 18446744073709551615u64 }"),
        Default::default(),
    ).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let mut session = BrowserSession::new().unwrap();
    assert_eq!(response(session.load_artifact(&bytes))["status"], "ready");
    let result = finish(&mut session);
    assert_eq!(result["output"], json!(["artifact hello 😀"]));
    assert_eq!(
        result["result"],
        json!({"tag":"u64", "value":"18446744073709551615"})
    );
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/browser-basic.kbc");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
