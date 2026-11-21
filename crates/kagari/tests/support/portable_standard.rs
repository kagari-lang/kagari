//! Author against verified core contracts from the host's chosen portable artifact.
use kagari::BytecodeArtifact;
use kagari_common::identity::map::DefinitionContext;
use kagari_runtime::native::catalog::DeclarationCatalog;
use kagari_stdlib::{StandardLibrary, namespaces};
use std::error::Error;

pub fn from_bytes(bytes: &[u8]) -> Result<StandardLibrary, Box<dyn Error>> {
    let artifact = BytecodeArtifact::from_bytes(bytes)?;
    let cancel = Default::default();
    artifact
        .records()
        .validate_resource_limits(artifact.definitions(), &cancel)?;
    let context = DefinitionContext::new()?;
    let verified = artifact
        .import_into(&context, &cancel)?
        .into_records()
        .into_verified(&Default::default(), &context, &cancel)?;
    // Project only authored owners. The artifact's application modules and Native
    // requirements cannot contribute definitions to the standard library.
    let owners = ["cmp", "convert", "default", "ops", "option", "result"]
        .map(|name| namespaces::module("core", name));
    let seed = DeclarationCatalog::default().with_verified_modules(verified.bytecode(), &owners)?;
    Ok(StandardLibrary::new(&seed)?)
}
