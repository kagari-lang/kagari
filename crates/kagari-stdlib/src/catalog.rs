//! Immutable declaration projections of the production standard-library bundle.
//! Semantic consumers never request native preparation.
// Keep the same children when this provider is shared by Runtime unit tests.
#[cfg_attr(test, path = "catalog/documentation.rs")]
pub(crate) mod documentation;
// Keep the same children when this provider is shared by Runtime unit tests.
#[cfg_attr(test, path = "catalog/semantic.rs")]
pub(crate) mod semantic;
use crate::assembly;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_types::declaration::module::{DeclarationError, ModuleDecl};
use std::sync::Arc;

/// Immutable production declarations from the same bundle that owns native preparation.
pub fn declarations() -> Result<Vec<DefinitionMetadata<ModuleDecl>>, DeclarationError> {
    assembly::semantic()
        .map(|semantic| semantic.snapshot().modules().to_vec())
        .map_err(|error| DeclarationError(error.to_string()))
}

pub fn shared() -> Result<Vec<Arc<DefinitionMetadata<ModuleDecl>>>, DeclarationError> {
    assembly::semantic()
        .map(|semantic| semantic.shared().to_vec())
        .map_err(|error| DeclarationError(error.to_string()))
}
