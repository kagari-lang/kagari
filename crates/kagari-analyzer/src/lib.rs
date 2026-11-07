//! Lattice-backed language-server composition and bounded verification entrypoints.
#[cfg(feature = "verification")]
use crate::protocol::verification as wire_verification;
#[cfg(any(test, feature = "verification"))]
use crate::session::verification as session_verification;
use std::{io, process::ExitCode};

mod documents;
mod logging;
mod protocol;
mod server;
mod session;
mod transport;

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
