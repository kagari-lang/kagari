//! Package selection is validated before the engine publishes its native catalog.
use super::{EngineConfig, KagariEngine};
use kagari_runtime::{error::RuntimeError, library::collections, native::module::NativeModule};

pub struct KagariEngineBuilder {
    config: EngineConfig,
    modules: Vec<NativeModule>,
    default_modules: bool,
}
impl Default for KagariEngineBuilder {
    fn default() -> Self {
        Self {
            config: EngineConfig::default(),
            modules: vec![],
            default_modules: true,
        }
    }
}
impl KagariEngineBuilder {
    /// Enable optional bundled algorithms. Language contracts and foundational
    /// collections remain available when this is false.
    pub fn default_modules(mut self, enabled: bool) -> Self {
        self.default_modules = enabled;
        self
    }

    pub fn config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }
    pub fn install(mut self, module: NativeModule) -> Self {
        self.modules.push(module);
        self
    }
    pub fn build(mut self) -> Result<KagariEngine, RuntimeError> {
        if self.default_modules {
            self.modules.insert(0, collections::module()?);
        }
        KagariEngine::with_native_modules(self.config, self.modules)
    }
}
