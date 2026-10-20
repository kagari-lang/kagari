//! Browser-owned, bounded JSON adapter over the SDK's validated interpreter path.
mod diagnostics;
mod output;
mod protocol;
#[cfg(test)]
mod tests;

use crate::{
    output::Output,
    protocol::{OwnedResult, Response, WebError},
};
#[cfg(feature = "source")]
use kagari::error::EmbeddingError;
use kagari::{
    BytecodeArtifact,
    engine::KagariEngine,
    program::PreparedProgram,
    runtime::{KagariRuntime, owned::DriveResult},
};
use kagari_runtime::{
    module::{LoadedModule, entry::ProgramEntry},
    session::owned::OwnedExecution,
};
#[cfg(feature = "source")]
use kagari_source::source::SourceFile;
use serde_json::to_string;
use std::{
    fmt::Display,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use wasm_bindgen::prelude::wasm_bindgen;

const ARTIFACT_BYTES: usize = 16 * 1024 * 1024;
#[cfg(feature = "source")]
const SOURCE_BYTES: usize = 256 * 1024;
const MAX_SLICE: u32 = 100_000;

/// One runtime and at most one active root, confined to its owning browser Worker.
/// Ordinary errors are JSON; construction errors throw a bounded JSON string in JS.
/// A Rust panic or fatal WASM fault invalidates the entire instance.
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
pub struct BrowserSession {
    engine: KagariEngine,
    runtime: KagariRuntime,
    module: Option<LoadedModule>,
    active: Option<OwnedExecution>,
    output: Arc<Mutex<Output>>,
}

#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
impl BrowserSession {
    /// Install only the standard library and the bounded `web::print(String)` sink.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        wasm_bindgen(constructor)
    )]
    pub fn new() -> Result<BrowserSession, String> {
        let output = Arc::new(Mutex::new(Output::default()));
        let engine = output::engine(output.clone()).map_err(|error| {
            to_string(&Response::error(WebError::sdk(&error))).expect("JSON records")
        })?;
        let runtime = engine.runtime(Default::default());
        Ok(Self {
            engine,
            runtime,
            module: None,
            active: None,
            output,
        })
    }

    /// Compile one `memory://main.kgr` directly to verified code and load a fresh runtime.
    /// Rejected candidates leave the previously loaded program intact.
    #[cfg(feature = "source")]
    pub fn compile(&mut self, source: &str) -> String {
        if self.active.is_some() {
            return self.reject(
                "WEB_BUSY",
                "cancel/drive or reset the active run before compiling",
            );
        }
        if source.len() > SOURCE_BYTES {
            return self.reject("WEB_SOURCE_LIMIT", "source exceeds 262144 UTF-8 bytes");
        }
        // A fresh analysis session bounds retained source history between requests.
        let engine = self.engine.new_session(Default::default());
        let program = match engine.compile_to_program(
            SourceFile::new("memory://main.kgr", source),
            Default::default(),
        ) {
            Ok(program) => program,
            Err(error) => {
                let mut response = Response::error(WebError::sdk(&error));
                if let EmbeddingError::Diagnostics {
                    diagnostics: records,
                } = &error
                {
                    let snapshot = engine.source_snapshot();
                    response.diagnostics = records
                        .iter()
                        .take(32)
                        .map(|record| diagnostics::convert(record, &snapshot))
                        .collect();
                    response.diagnostics_truncated = records.len() > 32;
                }
                return self.respond(response);
            }
        };
        self.load(program)
    }

    /// Decode and verify a bounded current artifact without requiring source support.
    pub fn load_artifact(&mut self, bytes: &[u8]) -> String {
        if self.active.is_some() {
            return self.reject(
                "WEB_BUSY",
                "cancel/drive or reset the active run before loading",
            );
        }
        if bytes.len() > ARTIFACT_BYTES {
            return self.reject("WEB_ARTIFACT_LIMIT", "artifact exceeds 16777216 bytes");
        }
        let artifact = match BytecodeArtifact::from_bytes(bytes) {
            Ok(artifact) => artifact,
            Err(error) => return self.reject("WEB_ARTIFACT_DECODE", &error),
        };
        match PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()) {
            Ok(program) => self.load(program),
            Err(error) => self.reject("WEB_ARTIFACT_VALIDATION", error),
        }
    }

    /// Queue the verified, zero-argument root main capability, not a public export alias.
    pub fn start(&mut self) -> String {
        if self.active.is_some() {
            return self.reject("WEB_BUSY", "one run is already active");
        }
        let Some(module) = &self.module else {
            return self.reject("WEB_NO_PROGRAM", "compile or load an artifact first");
        };
        match self
            .runtime
            .start(module, ProgramEntry::Main, &[], &Default::default())
        {
            Ok(active) => {
                self.active = Some(active);
                self.respond(Response::new("runnable"))
            }
            Err(error) => self.respond(Response::error(WebError::sdk(&error))),
        }
    }

    /// Execute 1..=100000 VM instructions; the caller owns event-loop scheduling.
    /// Waiting is reported without registering any arbitrary JS callback.
    pub fn drive(&mut self, slice: u32) -> String {
        if slice == 0 || slice > MAX_SLICE {
            return self.reject("WEB_SLICE_LIMIT", "slice must be in 1..=100000");
        }
        let Some(active) = &self.active else {
            return self.reject("WEB_NO_RUN", "start a run first");
        };
        let response = match self.runtime.drive(
            active,
            NonZeroUsize::new(slice as usize).expect("positive slice"),
        ) {
            Ok(DriveResult::Runnable) => Response::new("runnable"),
            Ok(DriveResult::Waiting) => Response::new("waiting"),
            Ok(DriveResult::Complete(result)) => {
                self.active.take();
                match result {
                    Ok(root) => match OwnedResult::rooted(&root, self.runtime.runtime().gc()) {
                        Ok(result) => Response {
                            result: Some(result),
                            ..Response::new("complete")
                        },
                        Err(error) => Response::error(error),
                    },
                    Err(error) => Response::error(WebError::sdk(&error)),
                }
            }
            Err(error) => {
                // Keep the owner available for explicit reset/retirement on API errors.
                Response::error(WebError::sdk(&error))
            }
        };
        self.respond(response)
    }

    /// Request cooperative cancellation; continue driving to observe terminal cleanup.
    pub fn cancel(&mut self) {
        if let Some(active) = &self.active {
            active.cancel();
        }
    }

    /// Cancel and retire the normal owner, drain cleanup and release the loaded program.
    /// This cannot recover a Rust panic or a fatal WASM fault; terminate the Worker then.
    pub fn reset(&mut self) -> String {
        self.cancel();
        self.active.take();
        if let Err(error) = self.runtime.drain_retired_executions() {
            return self.respond(Response::error(WebError::sdk(&error)));
        }
        self.module = None;
        self.runtime = self.engine.runtime(Default::default());
        self.respond(Response::new("idle"))
    }

    /// Drain pending output without advancing execution.
    pub fn drain_output(&mut self) -> String {
        self.respond(Response::new("output"))
    }
}

impl BrowserSession {
    fn load(&mut self, program: PreparedProgram) -> String {
        let mut runtime = self.engine.runtime(Default::default());
        match runtime.load_program(&program, Default::default()) {
            Ok(module) => {
                self.module = Some(module);
                self.runtime = runtime;
                self.respond(Response::new("ready"))
            }
            Err(error) => self.respond(Response::error(WebError::sdk(&error))),
        }
    }

    fn reject(&mut self, code: &str, message: impl Display) -> String {
        self.respond(Response::error(WebError::new(code, message)))
    }

    fn respond(&mut self, mut response: Response) -> String {
        match self.output.lock() {
            Ok(mut output) => (response.output, response.output_truncated) = output.drain(),
            Err(_) => {
                response.status = "error";
                response.error = Some(WebError::new(
                    "WEB_OUTPUT_POISONED",
                    "output queue poisoned; replace the instance",
                ));
            }
        }
        to_string(&response).expect("bounded JSON records")
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        self.cancel();
        self.active.take();
        // Explicit reset reports cleanup failures; Drop cannot report a JSON response.
        let _ = self.runtime.drain_retired_executions();
    }
}
