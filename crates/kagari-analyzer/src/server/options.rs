//! Validated process options and finite application budgets.
use crate::protocol::output::{
    MESSAGE_BASE_CHARGE_BYTES, REQUEST_LIFECYCLE_FRAMES, SESSION_CONTROL_RESERVE_FRAMES,
};
use clap::Parser;
use std::{
    io::{self, Error},
    net::SocketAddr,
    time::Duration,
};
#[cfg(test)]
use tokio::io::DuplexStream;

#[derive(Debug)]
pub(crate) enum Endpoint {
    Stdio,
    Connect(SocketAddr),
    Listen(SocketAddr),
    #[cfg(test)]
    Memory(DuplexStream),
}

#[derive(Clone, Debug)]
pub(crate) struct SessionLimits {
    pub ingress_count: usize,
    pub ingress_bytes: usize,
    pub open_file_bytes: usize,
    pub open_total_bytes: usize,
    pub open_documents: usize,
    pub document_uri_bytes: usize,
    pub document_metadata_bytes: usize,
    pub retained_input_bytes: usize,
    pub requests: usize,
    pub outbox_count: usize,
    pub outbox_bytes: usize,
    pub result_bytes: usize,
    pub publication_uris: usize,
    pub publication_diagnostics: usize,
    pub publication_bytes: usize,
    pub startup: Duration,
    pub write: Duration,
    pub cleanup: Duration,
    pub exit: Duration,
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            ingress_count: 8,
            ingress_bytes: 64 * 1024 * 1024,
            open_file_bytes: 4 * 1024 * 1024,
            open_total_bytes: 64 * 1024 * 1024,
            open_documents: 1024,
            document_uri_bytes: 8 * 1024,
            document_metadata_bytes: 256 * 1024,
            retained_input_bytes: 256 * 1024 * 1024,
            requests: 64,
            outbox_count: 256,
            outbox_bytes: 64 * 1024 * 1024,
            result_bytes: 8 * 1024 * 1024,
            publication_uris: 1024,
            publication_diagnostics: 16_384,
            publication_bytes: 16 * 1024 * 1024,
            startup: Duration::from_secs(10),
            write: Duration::from_secs(5),
            cleanup: Duration::from_secs(2),
            exit: Duration::from_secs(10),
        }
    }
}

impl SessionLimits {
    pub fn validate(&self) -> io::Result<()> {
        if self.ingress_count == 0
            || self.ingress_bytes < 32 * 1024 * 1024
            || self.ingress_bytes > u32::MAX as usize
            || self.open_file_bytes > self.open_total_bytes
            || self.open_documents == 0
            || self.document_uri_bytes == 0
            || self.document_metadata_bytes < self.document_uri_bytes
            || self.retained_input_bytes == 0
            || self.requests == 0
            || self.publication_uris == 0
            || self.publication_diagnostics == 0
            || self.publication_bytes < self.result_bytes
            || self.outbox_count
                < self
                    .requests
                    .saturating_mul(REQUEST_LIFECYCLE_FRAMES)
                    .saturating_add(SESSION_CONTROL_RESERVE_FRAMES)
            || self.outbox_bytes
                < self.result_bytes
                    + (self.requests * REQUEST_LIFECYCLE_FRAMES + SESSION_CONTROL_RESERVE_FRAMES)
                        * MESSAGE_BASE_CHARGE_BYTES
            || [self.startup, self.write, self.cleanup, self.exit]
                .iter()
                .any(Duration::is_zero)
        {
            return Err(Error::other("invalid session resource limits"));
        }
        Ok(())
    }
}

#[derive(Parser)]
#[command(name = "kagari-analyzer", version, about = "Kagari language server")]
#[group(id = "transport", multiple = false)]
struct Options {
    /// Communicate over standard input/output (the default).
    #[arg(long)]
    stdio: bool,
    /// Connect to an editor-owned TCP listener.
    #[arg(long, value_name = "ADDRESS", value_parser = loopback_address)]
    tcp_connect: Option<SocketAddr>,
    /// Accept one TCP client and run one session.
    #[arg(long, value_name = "ADDRESS", value_parser = loopback_address)]
    tcp_listen: Option<SocketAddr>,
}

pub(crate) fn command_line() -> Endpoint {
    match Options::parse() {
        Options { stdio: true, .. } => Endpoint::Stdio,
        Options {
            tcp_connect: Some(address),
            ..
        } => Endpoint::Connect(address),
        Options {
            tcp_listen: Some(address),
            ..
        } => Endpoint::Listen(address),
        _ => Endpoint::Stdio,
    }
}

fn loopback_address(value: &str) -> Result<SocketAddr, String> {
    let address = value
        .parse::<SocketAddr>()
        .map_err(|error| error.to_string())?;
    if address.ip().is_loopback() {
        Ok(address)
    } else {
        Err("TCP requires a numeric loopback address".into())
    }
}
