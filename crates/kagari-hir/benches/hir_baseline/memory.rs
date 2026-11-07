//! Requested-byte lifecycle driver; instrumented durations are not speed evidence.

mod allocation;
mod report;
mod scenarios;
mod workloads;

use serde_json::{json, to_value};

use crate::{
    allocation::{Allocator, Counts},
    report::{Options, measure, print},
    scenarios::{Setup, lifecycle},
};

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn main() {
    let options = Options::parse();
    if options.inputs {
        print(&options.manifest(true));
        return;
    }
    let (setup, provider_setup_ns) = measure(Setup::initialize);
    let mut checkpoints = [("", Counts::default()); 6];
    let mut index = 0;
    let result = lifecycle(&options.workload, &setup, |name| {
        if name == "baseline" {
            allocation::begin_window();
        }
        checkpoints[index] = (name, allocation::snapshot());
        index += 1;
    });
    assert_eq!(index, checkpoints.len());
    for (_, counts) in checkpoints {
        assert_eq!(
            counts.requested_bytes - counts.freed_bytes,
            counts.live_bytes
        );
        assert!(counts.peak_bytes >= counts.live_bytes);
    }
    for pair in checkpoints.windows(2) {
        assert!(pair[1].1.requests >= pair[0].1.requests);
        assert!(pair[1].1.requested_bytes >= pair[0].1.requested_bytes);
        assert!(pair[1].1.frees >= pair[0].1.frees);
        assert!(pair[1].1.freed_bytes >= pair[0].1.freed_bytes);
        assert!(pair[1].1.peak_bytes >= pair[0].1.peak_bytes);
    }
    assert!(
        result
            .metrics
            .iter()
            .all(|(_, ns)| ns.is_finite() && *ns >= 0.0)
    );
    let checkpoints: Vec<_> = checkpoints
        .into_iter()
        .map(|(name, counts)| {
            let mut value = to_value(counts).unwrap();
            value["name"] = json!(name);
            value
        })
        .collect();
    let mut evidence = result.evidence(&options.workload);
    evidence["allocation_accounting"] = json!(
        "process-cumulative successful requests and freed layouts via System; successful realloc counts one new request and one old-layout free; peak starts at baseline live bytes; not RSS"
    );
    evidence["timing_is_speed_evidence"] = json!(false);
    print(&options.result(
        true,
        provider_setup_ns,
        vec![json!({
            "name": "lifecycle", "checkpoints": checkpoints, "evidence": evidence,
        })],
    ));
}
