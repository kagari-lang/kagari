//! Package selection is validated before the engine publishes its native catalog.
use super::{EngineConfig, KagariEngine};
use kagari_runtime::{error::RuntimeError, native::module::NativeModule};

#[derive(Default)]
pub struct KagariEngineBuilder {
    config: EngineConfig,
    modules: Vec<NativeModule>,
}
impl KagariEngineBuilder {
    pub fn config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }
    pub fn install(mut self, module: NativeModule) -> Self {
        self.modules.push(module);
        self
    }
    pub fn build(self) -> Result<KagariEngine, RuntimeError> {
        KagariEngine::with_native_modules(self.config, self.modules)
    }
}
