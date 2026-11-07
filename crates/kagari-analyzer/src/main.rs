//! Executable owner of one language-server process.
use std::process::ExitCode;

fn main() -> ExitCode {
    match kagari_analyzer::run() {
        Ok(status) => status,
        Err(error) => {
            eprintln!("kagari-analyzer: {error}");
            ExitCode::FAILURE
        }
    }
}
