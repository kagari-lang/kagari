//! Thin launch adapter; Kagari semantics and recovery belong to the analyzer/client.

use zed_extension_api::{
    self as zed, Command, LanguageServerId, Result, Worktree, settings::LspSettings,
};

struct Kagari;

impl zed::Extension for Kagari {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        server: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        let settings = LspSettings::for_worktree(server.as_ref(), worktree)?;
        let binary = settings.binary.ok_or_else(|| {
            "Set lsp.kagari.binary.path to the absolute kagari-analyzer executable path".to_owned()
        })?;
        let command = binary.path.ok_or_else(|| {
            "Set lsp.kagari.binary.path to the absolute kagari-analyzer executable path".to_owned()
        })?;
        if !absolute(&command) {
            return Err("lsp.kagari.binary.path must be an absolute executable path".to_owned());
        }
        Ok(Command {
            command,
            args: binary
                .arguments
                .unwrap_or_else(|| vec!["--stdio".to_owned()]),
            env: Vec::new(),
        })
    }
}

// This code executes under WASI even on Windows; std::path uses the WASI platform,
// so it cannot validate a Windows host path. Do not search the worktree or PATH.
fn absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

zed::register_extension!(Kagari);
