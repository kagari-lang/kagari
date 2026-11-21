//! Standard native providers composed with caller-supplied verified core declarations.
mod assembly;
mod catalog;
mod foundation;

pub mod declarations;
pub mod identity;
pub mod namespaces;

use crate::{assembly::StandardBundle, declarations::StandardDeclarations};
use kagari_runtime::native::{
    binding::NativeResult, catalog::DeclarationCatalog, module::NativeModule,
};

/// A sealed standard library. Construction checks declarations without preparing callbacks.
pub struct StandardLibrary {
    bundle: StandardBundle,
    declarations: StandardDeclarations,
}

impl StandardLibrary {
    /// Compose the native owners and facades against the caller's verified core owners.
    pub fn new(seed: &DeclarationCatalog) -> NativeResult<Self> {
        let bundle = assembly::standard(seed);
        let declarations = StandardDeclarations::new(bundle.semantic()?);
        Ok(Self {
            bundle,
            declarations,
        })
    }

    /// Prepare native implementations once for this bundle. Does not install a runtime.
    pub fn modules(&self) -> NativeResult<Vec<NativeModule>> {
        self.bundle.modules().map(|modules| modules.to_vec())
    }

    /// Borrow the sealed semantic view independently of native preparation.
    pub fn declarations(&self) -> &StandardDeclarations {
        &self.declarations
    }
}

#[cfg(test)]
mod test_library;
#[cfg(test)]
mod tests;
