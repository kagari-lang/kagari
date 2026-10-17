//! Reusable registration preparation, independent of any script runtime.
use crate::{
    Runtime, RuntimeConfig,
    host::{HostRegistry, installation::PreparedHostTypes},
    native::{binding::NativeResult, module::NativeModule, registry::NativeRegistry},
};
use std::sync::Arc;

/// A closed native module set checked by the transactional installer.
///
/// Clones share registration records and Rust callbacks, including any state those
/// callbacks explicitly capture. They do not share script heaps, loaded modules,
/// host registries, borrow state or execution sessions. Prepare a separate set when
/// application callbacks must own different captured state.
/// Checked host type/path maps retain this installation's definition scope and are
/// copied into each runtime; runtime creation does not repeat their preparation.
#[derive(Debug, Clone)]
pub struct NativeInstallation {
    prepared: Arc<PreparedInstallation>,
}

#[derive(Debug)]
struct PreparedInstallation {
    registry: NativeRegistry,
    host_types: PreparedHostTypes,
}

impl NativeInstallation {
    /// Validate the entire registration closure before publishing reusable records.
    pub fn prepare(modules: &[NativeModule]) -> NativeResult<Self> {
        let mut registry = NativeRegistry::default();
        NativeModule::install_batch(modules, &mut registry)?;
        let host_types = HostRegistry::new(registry.catalog.types.context().clone(), None)
            .prepare_native_registration(&registry)?;
        Ok(Self {
            prepared: Arc::new(PreparedInstallation {
                registry,
                host_types,
            }),
        })
    }

    /// Create an independent runtime. Later registrations affect only that runtime;
    /// loading still validates each program against its installed contracts.
    pub fn runtime(&self, config: RuntimeConfig) -> Runtime {
        Runtime::with_native_registry(
            config,
            self.prepared.registry.clone(),
            Some(&self.prepared.host_types),
        )
    }
}
