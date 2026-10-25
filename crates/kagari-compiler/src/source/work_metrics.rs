//! Thread-local work counts for source-program scheduling tests.
//!
//! Declare this module only under `cfg(test)`. Events count actual work, not
//! retained output copied into a candidate. Reset immediately before one lowering
//! call; source checking, history preparation and encoding are outside that scope.

use std::{cell::Cell, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub initializations: usize,
    pub bodies: usize,
    pub candidates: usize,
    pub rounds: usize,
    pub scanned_bodies: usize,
    pub candidate_work: Duration,
}

thread_local! {
    static COUNTS: Cell<Snapshot> = const { Cell::new(Snapshot {
        initializations: 0,
        bodies: 0,
        candidates: 0,
        rounds: 0,
        scanned_bodies: 0,
        candidate_work: Duration::ZERO,
    }) };
}

/// Start an independent lowering measurement on this test thread.
pub(crate) fn reset() {
    COUNTS.set(Snapshot::default());
}

pub(crate) fn snapshot() -> Snapshot {
    COUNTS.get()
}

/// Count initialization of one authoring module's lowering state.
pub(crate) fn initialize() {
    let mut counts = snapshot();
    counts.initializations += 1;
    COUNTS.set(counts);
}

/// Count generation of one MIR body, including generated adapters and bridges.
pub(crate) fn body() {
    let mut counts = snapshot();
    counts.bodies += 1;
    COUNTS.set(counts);
}

/// Count a complete program candidate submitted for independent verification.
pub(crate) fn candidate() {
    let mut counts = snapshot();
    counts.candidates += 1;
    COUNTS.set(counts);
}

/// Count first-time callable/construction extraction for a completed body.
pub(crate) fn scanned_body() {
    let mut counts = snapshot();
    counts.scanned_bodies += 1;
    COUNTS.set(counts);
}

/// Accumulate candidate snapshot, evidence, optimization and verification time.
pub(crate) fn candidate_work(elapsed: Duration) {
    let mut counts = snapshot();
    counts.candidate_work += elapsed;
    COUNTS.set(counts);
}

/// Count an outer scheduling/demand-discovery pass, not an individual queue pop.
pub(crate) fn round() {
    let mut counts = snapshot();
    counts.rounds += 1;
    COUNTS.set(counts);
}
