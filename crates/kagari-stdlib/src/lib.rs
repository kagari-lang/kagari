//! Concrete standard declarations and native implementations installed by the host.
mod bindings;
pub mod catalog;
mod collections;
pub mod declarations;
pub mod identity;
pub mod namespaces;
use kagari_runtime::native::{binding::NativeResult, module::NativeModule};

/// Complete standard modules, ready for ordinary batch registration.
pub fn modules() -> NativeResult<Vec<NativeModule>> {
    bindings::modules()
}
