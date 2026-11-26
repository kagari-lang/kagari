//! Explicit importer requirements select an independently verified public owner.
use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, builder::ModuleBuilder, types::TraitRef},
};
use kagari_common::identity::DefinitionPath;

impl ModuleBuilder {
    /// Pin a public source trait from the builder's `with_verified_modules` catalog.
    ///
    /// The installed importer requires the exact portable trait ABI even if no
    /// method is bound or called. The declaration itself does not define the trait
    /// or grant access to it. Unknown, non-source, ineligible or duplicate paths fail.
    pub fn reference_trait(&mut self, declaration: &DefinitionPath) -> NativeResult<TraitRef> {
        let reference = self.providers.trait_reference(declaration)?;
        if !self
            .providers
            .is_source_requirement(reference.id, &self.declaration.identity)
            || !self.required_source_traits.insert(declaration.clone())
        {
            return Err(RuntimeError::metadata_conflict(
                "invalid or duplicate verified public source trait requirement",
            ));
        }
        Ok(reference)
    }
}
