//! B06 separates a terminal reply from actual retirement of running worker credit.
use crate::{
    server::options::SessionLimits,
    session::analysis::testing::CompletionGate,
    tests::{features::site, peer::Peer},
};
use kagari_hir::analysis::work::milestone::BodyCheckGate;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    env, fs,
    path::Path,
    process,
    time::{Duration, Instant},
};
use tokio::runtime::Builder;

const URI: &str = "untitled:running-body-cancellation";
const BEFORE: &str = include_str!("../../../tests/fixtures/workspace/standalone_error.kgr");
const AFTER: &str = include_str!("../../../tests/fixtures/workspace/standalone.kgr");

#[derive(Serialize)]
struct Sample {
    client_cancel_to_terminal_ms: f64,
    client_cancel_to_credit_completion_upper_bound_ms: f64,
    post_release_credit_retirement_upper_bound_ms: f64,
    controlled_pause_ms: f64,
    generation: u64,
    job: u64,
}

fn run_case() -> Sample {
    let (diagnostics, held) = CompletionGate::new();
    let (feature, started, release) = BodyCheckGate::new();
    let mut peer = Peer::with_feature_gate(
        env!("CARGO_MANIFEST_DIR").into(),
        SessionLimits::default(),
        diagnostics,
        feature,
    );
    Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // A failing assertion drops the release sender before the outer peer
            // waits for owned worker cleanup.
            let resume = release;
            let initialize = peer
                .request(1, "initialize", json!({"capabilities":{}}))
                .await;
            assert_eq!(initialize["result"]["capabilities"]["hoverProvider"], true);
            peer.notify("initialized", json!({})).await;
            peer.notify(
                "textDocument/didOpen",
                json!({"textDocument":{
                    "uri":URI, "languageId":"kagari", "version":1, "text":BEFORE
                }}),
            )
            .await;
            peer.capture(2).await;
            let previous = held
                .recv_timeout(Duration::from_secs(5))
                .expect("prior real diagnostics hold worker");
            peer.notify(
                "textDocument/didChange",
                json!({
                    "textDocument":{"uri":URI,"version":2}, "contentChanges":[{"text":AFTER}]
                }),
            )
            .await;
            peer.capture(3).await;
            peer.begin_request(2, "textDocument/hover", site(URI, AFTER, "7"))
                .await;
            previous.release.send(()).unwrap();
            let body_started = started
                .recv_timeout(Duration::from_secs(5))
                .expect("selected checker entered new body");
            let (job, generation) = peer.feature_started().await;
            assert_eq!(generation, 3);
            assert!(job > previous.job);
            assert!(peer.completed_job() < job);

            let cancelled = Instant::now();
            peer.notify("$/cancelRequest", json!({"id":2})).await;
            let terminal = peer.response(2).await;
            let terminal_ms = cancelled.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(terminal["error"]["code"], -32800, "{terminal}");
            // The request has a terminal decision, but its blocked worker still owns
            // the unique credit. Cancelling must never free that credit early.
            assert!(peer.completed_job() < job);

            let resumed = Instant::now();
            let controlled_pause_ms = body_started.elapsed().as_secs_f64() * 1000.0;
            resume.send(()).unwrap();
            peer.handled(job).await;
            let retirement_ms = resumed.elapsed().as_secs_f64() * 1000.0;
            let cancellation_retirement_ms = cancelled.elapsed().as_secs_f64() * 1000.0;
            // Monotonic exact-completion accounting means this observation is an
            // upper bound on this job's retirement even if a later job also finishes.
            assert!(peer.completed_job() >= job);
            peer.shutdown().await;

            Sample {
                client_cancel_to_terminal_ms: terminal_ms,
                client_cancel_to_credit_completion_upper_bound_ms: cancellation_retirement_ms,
                post_release_credit_retirement_upper_bound_ms: retirement_ms,
                controlled_pause_ms,
                generation,
                job,
            }
        })
}

fn percentiles(samples: &[Sample], metric: fn(&Sample) -> f64) -> Value {
    if samples.len() < 20 {
        return Value::Null;
    }
    let mut values: Vec<_> = samples.iter().map(metric).collect();
    values.sort_by(f64::total_cmp);
    json!({
        "p50": values[(values.len() * 50).div_ceil(100) - 1],
        "p95": values[(values.len() * 95).div_ceil(100) - 1]
    })
}

#[test]
fn running_selected_body_cancellation_reports_terminal_before_worker_retirement() {
    let count = match env::var("KAGARI_LSP_CANCEL_SAMPLES") {
        Ok(value) => value
            .parse::<usize>()
            .expect("KAGARI_LSP_CANCEL_SAMPLES must be an integer"),
        Err(env::VarError::NotPresent) => 1,
        Err(error) => panic!("cannot read cancellation sample count: {error}"),
    };
    assert!(
        (1..=200).contains(&count),
        "KAGARI_LSP_CANCEL_SAMPLES must be 1..200"
    );
    let samples: Vec<_> = (0..count).map(|_| run_case()).collect();
    let summaries = json!({
        "client_cancel_to_terminal": percentiles(&samples, |sample| sample.client_cancel_to_terminal_ms),
        "client_cancel_to_credit_completion_upper_bound": percentiles(&samples, |sample| sample.client_cancel_to_credit_completion_upper_bound_ms),
        "post_release_credit_retirement_upper_bound": percentiles(&samples, |sample| sample.post_release_credit_retirement_upper_bound_ms),
        "controlled_pause": percentiles(&samples, |sample| sample.controlled_pause_ms)
    });
    let report = json!({
        "metric":"B06", "mode":"controlled-running-selected-body", "samples":count,
        "profile":if cfg!(debug_assertions) {"dev-test-opt1"} else {"release-test"},
        "pid":process::id(), "os":env::consts::OS, "architecture":env::consts::ARCH,
        "clock":"one process monotonic Instant; no cross-clock subtraction",
        "cache":"fresh actor/workers/query state per sample; process and filesystem/native cache shared within group",
        "scope":"selected body entry milestone; no large-body intraloop or unpaused p95 claim",
        "percentile_method":"nearest rank; p50/p95 emitted only for at least 20 samples",
        "percentiles_ms":summaries, "raw_samples":samples,
        "targets_ms":{"client_cancel_to_terminal_p95":50,"client_cancel_to_credit_completion_upper_bound_p95":250},
        "unpaused_p95":false
    });
    let output = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lsp-benchmark/B06-running-cancel.json");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!(
        "B06 samples={count} percentiles_ms={summaries} report={}",
        output.display()
    );
}
