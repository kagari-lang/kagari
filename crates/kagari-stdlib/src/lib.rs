//! Concrete standard declarations and native implementations installed by the host.
// SR01 wiring: exercise the production bundle with representative recipes without
// switching legacy consumers. Enable unconditionally when SR02 connects real
// contributors; SR05 removes the remaining legacy cache owners, not this bundle.
#[cfg(test)]
mod assembly;

mod bindings;
pub mod catalog;
pub mod declarations;
pub mod identity;
pub mod namespaces;
use kagari_runtime::native::{binding::NativeResult, module::NativeModule};

/// Complete standard modules, ready for ordinary batch registration.
pub fn modules() -> NativeResult<Vec<NativeModule>> {
    bindings::modules()
}

#[cfg(test)]
mod tests;
