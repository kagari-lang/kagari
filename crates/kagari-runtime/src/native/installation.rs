//! Reusable registration preparation, independent of any script runtime.
use crate::{
    Runtime, RuntimeConfig,
    host::HostRegistry,
    native::{binding::NativeResult, module::NativeModule, registry::NativeRegistry},
};
use std::sync::Arc;

/// A closed native module set checked by the transactional installer.
///
/// Clones share registration records and Rust callbacks, including any state those
/// callbacks explicitly capture. They do not share script heaps, loaded modules,
/// host registries, borrow state or execution sessions. Prepare a separate set when
/// application callbacks must own different captured state.
#[derive(Debug, Clone)]
pub struct NativeInstallation {
    registry: Arc<NativeRegistry>,
}

impl NativeInstallation {
    /// Validate the entire registration closure before publishing reusable records.
    pub fn prepare(modules: &[NativeModule]) -> NativeResult<Self> {
        let mut registry = NativeRegistry::default();
        NativeModule::install_batch(modules, &mut registry)?;
        HostRegistry::new(registry.catalog.types.context().clone())
            .prepare_native_registration(&registry)?;
        Ok(Self {
            registry: Arc::new(registry),
        })
    }

    /// Create an independent runtime. Later registrations affect only that runtime;
    /// loading still validates each program against its installed contracts.
    pub fn runtime(&self, config: RuntimeConfig) -> Runtime {
        Runtime::with_native_registry(config, self.registry.as_ref().clone())
    }
}
