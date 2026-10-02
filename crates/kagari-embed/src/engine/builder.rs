//! Package selection is validated before the engine publishes its native catalog.
use super::{EngineConfig, KagariEngine};
use kagari_runtime::{error::RuntimeError, native::api::NativeApi};

#[derive(Default)]
pub struct KagariEngineBuilder {
    config: EngineConfig,
    packages: Vec<Result<NativeApi, RuntimeError>>,
}
impl KagariEngineBuilder {
    pub fn config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }
    pub fn install(mut self, package: Result<NativeApi, RuntimeError>) -> Self {
        self.packages.push(package);
        self
    }
    pub fn build(self) -> Result<KagariEngine, RuntimeError> {
        KagariEngine::with_native_apis(
            self.config,
            self.packages.into_iter().collect::<Result<_, _>>()?,
        )
    }
}
