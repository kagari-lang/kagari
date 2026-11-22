//! Explicit script output, copied only up to the remaining transport budget.
use crate::protocol::prefix;
#[cfg(not(feature = "source"))]
use kagari::BytecodeArtifact;
use kagari::{engine::KagariEngine, error::EmbeddingError};
#[cfg(not(feature = "source"))]
use kagari_common::identity::map::DefinitionContext;
#[cfg(not(feature = "source"))]
use kagari_runtime::error::RuntimeError;
use kagari_runtime::host::HostError;
use kagari_stdlib::{StandardLibrary, io};
#[cfg(not(feature = "source"))]
use std::error::Error;
use std::{
    mem,
    sync::{Arc, Mutex},
};

pub(crate) const OUTPUT_BYTES: usize = 64 * 1024;
pub(crate) const OUTPUT_MESSAGES: usize = 256;
pub(crate) const MESSAGE_BYTES: usize = 4096;

#[derive(Default)]
pub(crate) struct Output {
    messages: Vec<String>,
    bytes: usize,
    truncated: bool,
}

impl Output {
    fn push(&mut self, text: &str, suffix: &str) {
        if self.messages.len() == OUTPUT_MESSAGES || self.bytes == OUTPUT_BYTES {
            self.truncated = true;
            return;
        }
        let budget = MESSAGE_BYTES.min(OUTPUT_BYTES - self.bytes);
        let kept_text = prefix(text, budget);
        let kept_suffix = if kept_text.len() == text.len() {
            prefix(suffix, budget - kept_text.len())
        } else {
            ""
        };
        self.truncated |= kept_text.len() < text.len() || kept_suffix.len() < suffix.len();
        let kept_bytes = kept_text.len() + kept_suffix.len();
        let mut message = String::with_capacity(kept_bytes);
        message.push_str(kept_text);
        message.push_str(kept_suffix);
        self.bytes += kept_bytes;
        self.messages.push(message);
    }

    pub(crate) fn drain(&mut self) -> (Vec<String>, bool) {
        let previous = mem::take(self);
        (previous.messages, previous.truncated)
    }
}

#[cfg(not(feature = "source"))]
fn portable_library(bytes: &[u8]) -> Result<StandardLibrary, Box<dyn Error>> {
    let artifact = BytecodeArtifact::from_bytes(bytes)?;
    let cancel = Default::default();
    artifact
        .records()
        .validate_resource_limits(artifact.definitions(), &cancel)?;
    let context = DefinitionContext::new()?;
    let verified = artifact
        .import_into(&context, &cancel)?
        .into_records()
        .into_verified(&Default::default(), &context, &cancel)?;
    Ok(StandardLibrary::from_verified_program(verified.bytecode())?)
}

pub(crate) fn engine(
    output: Arc<Mutex<Output>>,
    #[cfg(not(feature = "source"))] seed: &[u8],
) -> Result<KagariEngine, EmbeddingError> {
    let registration = |error| EmbeddingError::Registration { error };
    let mut builder = KagariEngine::builder().map_err(registration)?;
    #[cfg(feature = "source")]
    let standard = StandardLibrary::new(builder.declarations()).map_err(registration)?;
    #[cfg(not(feature = "source"))]
    let standard = portable_library(seed)
        .map_err(|error| registration(RuntimeError::module_validation(error.to_string())))?;
    builder
        .install_all(standard.modules().map_err(registration)?)
        .map_err(registration)?;
    // The adapter owns this lexical root; memory:// inputs never access a filesystem.
    #[cfg(feature = "source")]
    builder.source_root("/kagari-browser")?;
    let module = io::module(
        standard.declarations().catalog(),
        move |context, text, suffix| {
            // Lock outside the payload loan and copy only the admitted prefix.
            let mut output = output
                .lock()
                .map_err(|_| HostError::new("output queue poisoned"))?;
            context
                .read_string(text, |text| output.push(text, suffix))
                .map_err(|error| HostError::new(error.to_string()))
        },
    )
    .map_err(registration)?;
    builder.install(module).map_err(registration)?;
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_utf8_bounded_and_drain_resets_the_budget() {
        let mut queue = Output::default();
        let text = "x".repeat(MESSAGE_BYTES - 1);
        queue.push(&format!("{text}😀"), "\n");
        assert_eq!(queue.messages[0], text);
        for _ in 1..OUTPUT_BYTES / MESSAGE_BYTES {
            queue.push(&text, "\n");
        }
        queue.push("", "\n");
        queue.push("not kept", "");
        assert_eq!(queue.bytes, OUTPUT_BYTES);
        let (messages, truncated) = queue.drain();
        assert!(truncated);
        assert_eq!(
            messages.iter().map(String::len).sum::<usize>(),
            OUTPUT_BYTES
        );
        assert_eq!(messages[1], format!("{text}\n"));
        assert_eq!(messages.last().unwrap(), "\n");
        for _ in 0..OUTPUT_MESSAGES + 1 {
            queue.push("", "");
        }
        let (messages, truncated) = queue.drain();
        assert_eq!(messages.len(), OUTPUT_MESSAGES);
        assert!(truncated);
        let full = "x".repeat(MESSAGE_BYTES);
        queue.push(&full, "\n");
        assert_eq!(queue.drain(), (vec![full], true));
        queue.push("new", "");
        assert_eq!(queue.drain(), (vec!["new".to_owned()], false));
    }
}
