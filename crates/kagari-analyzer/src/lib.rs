//! Lattice-backed language-server composition and bounded verification entrypoints.
#[cfg(any(test, feature = "verification"))]
use crate::analysis::verification as analysis_verification;
#[cfg(any(test, feature = "verification"))]
use crate::analysis::verification::retention as retention_verification;
#[cfg(any(test, feature = "verification"))]
use crate::documents::verification as document_verification;
#[cfg(feature = "verification")]
use crate::protocol::verification as wire_verification;
#[cfg(any(test, feature = "verification"))]
use crate::session::verification as session_verification;
use std::{io, process::ExitCode};

mod analysis;
mod documents;
mod logging;
mod protocol;
mod server;
mod session;
mod transport;
mod workspace;

#[cfg(test)]
mod tests;

/// Runs one server session using command-line transport options.
///
/// Standard output is reserved for protocol frames. IO and malformed startup
/// options return an error; protocol termination determines the process status.
pub fn run() -> io::Result<ExitCode> {
    server::run()
}

/// Replay a bounded wire input through the production codec and JSON-RPC oracle.
///
/// Available to verification consumers only; inputs over 64 KiB are skipped and
/// invariant failures remain panics for corpus replay and libFuzzer reporting.
#[cfg(feature = "verification")]
pub fn verify_wire(input: &[u8]) {
    wire_verification::check(input);
}

/// Replays bounded lifecycle actions against real request/output owners and an independent model.
/// Invalid transitions remain exercised; panics propagate to the corpus or fuzz runner.
#[cfg(any(test, feature = "verification"))]
pub fn verify_session(input: &[u8]) {
    session_verification::check(input);
}

/// Replays bounded full-sync documents and coordinates against a plain-string model.
/// Inputs over 64 KiB are skipped; invariant failures remain visible to replay and fuzzing.
#[cfg(any(test, feature = "verification"))]
pub fn verify_documents(input: &[u8]) {
    document_verification::check(input);
}

/// Compares bounded edited projects through reused and fresh semantic query owners.
/// Inputs over 1 KiB are skipped; mismatches and retained-input leaks remain panics.
#[cfg(any(test, feature = "verification"))]
pub fn verify_analysis(input: &[u8]) {
    analysis_verification::check(input);
}

/// Requested source/index ownership observed by the separate retention probe.
#[cfg(any(test, feature = "verification"))]
#[derive(Clone, Copy)]
pub struct RetentionCheckpoint {
    pub phase: &'static str,
    pub edits: usize,
    pub input_bytes: usize,
    pub index_bytes: usize,
    pub definition_records: usize,
    pub contexts: usize,
}

/// Exercises the real retained-query lifecycle while an external allocation probe samples it.
#[cfg(any(test, feature = "verification"))]
pub fn verify_retention(checkpoint: impl FnMut(RetentionCheckpoint)) {
    retention_verification::run(checkpoint);
}
