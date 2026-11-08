//! B07 allocation/retention and instrumented search-scope component probes.
mod allocation;

use allocation::{Allocator, Counts};
use kagari_analyzer::{RetentionCheckpoint, verify_retention, verify_scope_isolation};
use serde_json::json;
use std::{
    env,
    io::{self, Read, Write},
};

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn main() {
    if env::args().any(|argument| argument == "--scope-isolation") {
        let mut rows = Vec::new();
        verify_scope_isolation(|checkpoint| rows.push(checkpoint));
        println!(
            "{}",
            json!({
                "schema": 1, "workload": "B03+B04+B07.scope-isolation-v1",
                "measurement_layer": "analysis-and-acquisition-component",
                "actor_runtime": false, "transport": "none", "rss_measured": false,
                "checkpoints": rows,
            })
        );
        return;
    }
    let external = env::args().any(|argument| argument == "--external-checkpoints");
    let mut output = io::stdout().lock();
    let mut input = io::stdin().lock();
    let mut acknowledgement = [0];
    if external {
        // Prime both stdio buffers before the native/allocator baseline.
        writeln!(output, "ready").unwrap();
        output.flush().unwrap();
        input.read_exact(&mut acknowledgement).unwrap();
    }
    let mut checkpoints: [Option<(RetentionCheckpoint, Counts)>; 6] = [None; 6];
    let mut next = 0;
    verify_retention(|checkpoint| {
        if checkpoint.phase == "baseline" {
            allocation::begin_window();
        }
        let counts = allocation::snapshot();
        assert_eq!(
            counts.requested_bytes - counts.freed_bytes,
            counts.live_bytes
        );
        assert!(counts.peak_bytes >= counts.live_bytes);
        checkpoints[next] = Some((checkpoint, counts));
        next += 1;
        if external {
            writeln!(
                output,
                "checkpoint {} {}",
                checkpoint.phase, checkpoint.edits
            )
            .unwrap();
            output.flush().unwrap();
            input.read_exact(&mut acknowledgement).unwrap();
        }
    });
    assert_eq!(next, checkpoints.len());
    let rows = checkpoints.into_iter().map(|checkpoint| {
        let (sample, allocation) = checkpoint.unwrap();
        json!({"phase":sample.phase,"edits":sample.edits,"source_input_bytes":sample.input_bytes,"index_row_bytes":sample.index_bytes,"definition_records":sample.definition_records,"contexts":sample.contexts,"allocation":allocation})
    }).collect::<Vec<_>>();
    writeln!(output,
        "{}",
        json!({
            "schema":1,"workload":"lsp-retention-1000-v1","os":env::consts::OS,"arch":env::consts::ARCH,
            "measurement_layer":"analysis-and-acquisition-component","actor_runtime":false,"transport":"none",
            "external_checkpoints":external,
            "timing_is_speed_evidence":false,
            "allocation_accounting":"process-cumulative successful System requests/freed layouts; successful realloc counts a new request plus an old-layout free; peak starts at native baseline; not RSS",
            "checkpoints":rows,
        })
    ).unwrap();
}
