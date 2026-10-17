//! Stdio LSP foundation; language analysis and editor highlighting are separate products.
mod documents;
mod protocol;
mod server;
mod session;

#[cfg(test)]
mod tests;

use std::process::ExitCode;

fn main() -> ExitCode {
    match server::run_stdio() {
        Ok(status) => status,
        Err(error) => {
            eprintln!("kagari-analyzer: {error}");
            ExitCode::FAILURE
        }
    }
}
