//! Engine configuration and optional source compilation state.
#[cfg(feature = "source")]
pub(crate) mod source;

use crate::{context::ExecutionContext, runtime::KagariRuntime};
use kagari_abi::native_api::NativeApiSource;
#[cfg(feature = "source")]
use kagari_common::source_database::SourceDatabase;
#[cfg(feature = "source")]
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{NativeApi, Runtime, RuntimeConfig, RuntimeError, standard_library};
#[cfg(feature = "source")]
use std::cell::RefCell;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub default_runtime: RuntimeConfig,
    pub install_standard_library: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            default_runtime: RuntimeConfig::default(),
            install_standard_library: true,
        }
    }
}

#[derive(Debug)]
pub struct KagariEngine {
    config: EngineConfig,
    native_api: NativeApi,
    #[cfg(feature = "source")]
    sources: RefCell<SourceDatabase>,
    #[cfg(feature = "source")]
    analysis: RefCell<AnalysisDatabase>,
}

impl KagariEngine {
    pub fn new(config: EngineConfig) -> Self {
        Self::with_native_apis(config, vec![]).expect("default native API installation")
    }

    /// Built-in and application packages use the same checked installation path.
    pub fn with_native_apis(
        config: EngineConfig,
        mut native_apis: Vec<NativeApi>,
    ) -> Result<Self, RuntimeError> {
        if config.install_standard_library {
            native_apis.insert(0, standard_library());
        }
        let native_api = NativeApi::combine(native_apis)?;
        #[cfg(feature = "source")]
        let analysis = {
            let mut analysis = AnalysisDatabase::default();
            let modules = native_api.modules().to_vec();
            analysis.set_native_modules(modules, config.install_standard_library);
            RefCell::new(analysis)
        };
        Ok(Self {
            config,
            native_api,
            #[cfg(feature = "source")]
            sources: RefCell::default(),
            #[cfg(feature = "source")]
            analysis,
        })
    }

    pub fn native_declaration_sources(&self) -> Vec<NativeApiSource> {
        self.native_api.declaration_sources()
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn runtime(&self, context: ExecutionContext) -> KagariRuntime {
        let mut config = self.config.default_runtime.clone();
        config.security = context.security_context();
        config.host_exposure = context.host_policy.clone();
        config.resources = context.resources;
        let mut runtime = Runtime::new(config);
        self.native_api
            .install(&mut runtime)
            .expect("engine validated native bindings");
        KagariRuntime::new(runtime, context)
    }
}

impl Default for KagariEngine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}
