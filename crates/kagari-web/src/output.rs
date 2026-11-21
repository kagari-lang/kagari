//! Explicit script output, copied only up to the remaining transport budget.
use crate::protocol::prefix;
#[cfg(not(feature = "source"))]
use kagari::BytecodeArtifact;
use kagari::{engine::KagariEngine, error::EmbeddingError};
#[cfg(not(feature = "source"))]
use kagari_common::identity::map::DefinitionContext;
#[cfg(not(feature = "source"))]
use kagari_runtime::error::RuntimeError;
use kagari_runtime::{
    host::{HostCallContext, HostError},
    native::{builder::ModuleBuilder, declarations::FunctionDecl, host, types::Type},
    value::view::ValueView,
};
use kagari_stdlib::StandardLibrary;

use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
};
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
    pub(crate) fn push(&mut self, text: &str) {
        if self.messages.len() == OUTPUT_MESSAGES || self.bytes == OUTPUT_BYTES {
            self.truncated = true;
            return;
        }
        let kept = prefix(text, MESSAGE_BYTES.min(OUTPUT_BYTES - self.bytes));
        self.truncated |= kept.len() < text.len();
        self.bytes += kept.len();
        self.messages.push(kept.to_owned());
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
    let mut module = ModuleBuilder::new("web", builder.declarations()).map_err(registration)?;
    let print = module
        .define_function(
            FunctionDecl::new("print")
                .parameter("text", Type::scalar(BuiltinType::String))
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::Owned],
                    effects: HostFunctionEffects {
                        may_trap: true,
                        may_mutate_host_state: true,
                        ..Default::default()
                    },
                }),
        )
        .map_err(registration)?;
    module
        .bind(
            print,
            host::callback(
                move |context: &mut HostCallContext<'_>, args: &[ValueView<'_>]| {
                    let [text] = args else {
                        return Err(HostError::new("web::print requires String"));
                    };
                    // Lock outside the payload loan. Copy only the admitted output
                    // prefix; the whole script String stays in its original source.
                    let mut output = output
                        .lock()
                        .map_err(|_| HostError::new("output queue poisoned"))?;
                    context
                        .read_string(text, |text| output.push(text))
                        .map_err(|error| HostError::new(error.to_string()))?;
                    Ok(ValueView::unit())
                },
            ),
        )
        .map_err(registration)?;
    builder
        .install(module.finish().map_err(registration)?)
        .map_err(registration)?;
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_utf8_bounded_and_drain_resets_the_budget() {
        let mut queue = Output::default();
        queue.push(&"\u{1f600}".repeat(MESSAGE_BYTES));
        assert_eq!(queue.messages[0].len(), MESSAGE_BYTES);
        for _ in 0..OUTPUT_MESSAGES * 2 {
            queue.push(&"x".repeat(MESSAGE_BYTES));
        }
        assert_eq!(queue.bytes, OUTPUT_BYTES);
        let (messages, truncated) = queue.drain();
        assert!(truncated);
        assert_eq!(
            messages.iter().map(String::len).sum::<usize>(),
            OUTPUT_BYTES
        );
        for _ in 0..OUTPUT_MESSAGES + 1 {
            queue.push("");
        }
        let (messages, truncated) = queue.drain();
        assert_eq!(messages.len(), OUTPUT_MESSAGES);
        assert!(truncated);
        queue.push("new");
        assert_eq!(queue.drain(), (vec!["new".to_owned()], false));
    }
}
