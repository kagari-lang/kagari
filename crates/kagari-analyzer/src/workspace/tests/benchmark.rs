//! Acquisition component measurement over the existing frozen pilot recipe.
use crate::workspace::{
    CaptureRequest, MAX_INPUT_BYTES, WorkspaceCapture,
    acquisition::{self, AcquisitionPort},
    budget::InputBudget,
    origins::file_uri,
    tests::document_at,
};
use kagari_common::cancellation::CancellationToken;
use serde_json::{Value, json};
use std::{collections::BTreeSet, env, fs, path::PathBuf, process::Command, time::Instant};
use tokio::sync::oneshot;

fn measured_capture(port: &AcquisitionPort, request: CaptureRequest) -> (f64, WorkspaceCapture) {
    let (sender, receiver) = oneshot::channel();
    let start = Instant::now();
    port.submit(request, CancellationToken::default(), sender)
        .unwrap();
    let capture = receiver.blocking_recv().unwrap().unwrap();
    (start.elapsed().as_secs_f64() * 1000.0, capture)
}

fn assert_pilot(capture: &WorkspaceCapture, key: &str, expected: &str) {
    assert!(capture.errors.is_empty(), "{:?}", capture.errors);
    assert_eq!(capture.contexts.len(), 2);
    assert!(capture.contexts.iter().all(|context| context.complete
        && context.semantic_available
        && context.errors.is_empty()));
    let unique = capture
        .contexts
        .iter()
        .flat_map(|context| {
            context
                .origins
                .values()
                .map(|origin| origin.provider_key.clone())
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), 20);
    let binding = &capture.document_contexts[key];
    let context = capture
        .contexts
        .iter()
        .find(|context| context.key == binding.context)
        .unwrap();
    assert_eq!(context.sources.file(binding.file).unwrap().text(), expected);
    assert_eq!(
        context.origins[&binding.file].module.package,
        context.root.package
    );
}

#[test]
#[ignore = "controlled acquisition measurement; set KAGARI_LSP_ACQUISITION_BENCH to a materialized frozen pilot"]
fn acquisition_frozen_pilot() {
    let root = PathBuf::from(
        env::var_os("KAGARI_LSP_ACQUISITION_BENCH").expect("materialized frozen pilot path"),
    );
    let metadata: Value =
        serde_json::from_slice(&fs::read(root.join("measurement-manifest.json")).unwrap()).unwrap();
    let frozen: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/benchmark/manifest.json"
    ))
    .unwrap();
    assert_eq!(
        metadata["manifest_sha256"],
        frozen["workloads"]["pilot"]["manifest_sha256"]
    );
    let root_uri = file_uri(root.to_str().unwrap()).unwrap();
    let source_uri = file_uri(root.join("api/src/scale0.kgr").to_str().unwrap()).unwrap();
    let original = fs::read_to_string(root.join("api/src/scale0.kgr")).unwrap();
    let changed = original.replace(" + 1;", " + 2;");
    assert_ne!(original, changed);
    let variants = [
        document_at(source_uri.clone(), &original, 1),
        document_at(source_uri, &changed, 2),
    ];
    let mut groups = Vec::new();
    let mut max_retained_at_completion = 0;
    for group in 0..7 {
        let budget = InputBudget::new(MAX_INPUT_BYTES);
        let worker = acquisition::start(
            env::current_dir().unwrap().to_str().unwrap().into(),
            budget.clone(),
        )
        .unwrap();
        let mut samples = Vec::with_capacity(200);
        let mut first = 0.0;
        for step in 0..220 {
            let mut document = variants[step % 2].clone();
            document.version = i32::try_from(step + 1).unwrap();
            let expected = if step % 2 == 0 { &original } else { &changed };
            let request = CaptureRequest::new(
                u64::try_from(step + 1).unwrap(),
                vec![root_uri.clone()],
                vec![document.clone()],
                &budget,
            )
            .unwrap();
            let (milliseconds, capture) = measured_capture(&worker.port, request);
            assert_pilot(&capture, &document.source_name, expected);
            max_retained_at_completion = max_retained_at_completion.max(budget.retained_bytes());
            if step == 0 {
                first = milliseconds;
            }
            if step >= 20 {
                samples.push(milliseconds);
            }
        }
        drop(worker.port);
        worker.thread.join().unwrap();
        worker.terminated.blocking_recv().unwrap().unwrap();
        assert_eq!(budget.retained_bytes(), 0);
        samples.sort_by(f64::total_cmp);
        groups.push(json!({"group": group, "first_capture_ms": first, "samples": samples.len(), "p50_ms": samples[99], "p95_ms": samples[189]}));
    }
    let toolchain = Command::new("rustc").arg("--version").output().unwrap();
    assert!(toolchain.status.success());
    eprintln!(
        "acquisition-report {}",
        json!({
            "schema": 1,
            "component": "acquisition worker submit-to-owned-capture",
            "workload": "pilot",
            "manifest_sha256": metadata["manifest_sha256"],
            "toolchain": String::from_utf8(toolchain.stdout).unwrap().trim(),
            "os": env::consts::OS,
            "architecture": env::consts::ARCH,
            "debug_assertions": cfg!(debug_assertions),
            "profile": "workspace test; compilation excluded",
            "features": "default",
            "parallelism": "one acquisition worker per sequential group; Cargo default build parallelism",
            "cache": "fresh worker/context databases per group; 20 warmups; warm OS file cache",
            "max_charged_input_bytes_at_completion": max_retained_at_completion,
            "groups": groups,
            "scope": "source discovery, overlays, package scopes and source commits; excludes HIR, diagnostics and wire transport"
        })
    );
}
