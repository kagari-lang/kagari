//! Author against verified core contracts from the host's chosen portable artifact.
use kagari::BytecodeArtifact;
use kagari_common::identity::map::DefinitionContext;
use kagari_stdlib::StandardLibrary;
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
    Ok(StandardLibrary::from_verified_program(verified.bytecode())?)
}
