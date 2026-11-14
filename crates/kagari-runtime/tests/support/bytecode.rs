//! Prepare raw instruction fixtures before entering the canonical verified loader.
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    module::{InstalledProgram, VerifiedProgram},
    reload::ReloadValidationError,
};

pub(crate) fn load_bytecode(
    runtime: &mut Runtime,
    name: impl Into<String>,
    bytecode: DefinitionMetadata<BytecodeProgram>,
) -> Result<InstalledProgram, RuntimeError> {
    let context = DefinitionContext::new()
        .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
    let verified = VerifiedProgram::new(bytecode, &context)?;
    runtime.load_verified_program(name, verified)
}

#[allow(dead_code)] // Only reload lifecycle targets use this fixture operation.
pub(crate) fn reload_bytecode(
    runtime: &mut Runtime,
    active: &InstalledProgram,
    name: Option<String>,
    bytecode: DefinitionMetadata<BytecodeProgram>,
) -> Result<(), ReloadValidationError> {
    let staged = runtime.stage_reload_program(active, name, bytecode)?;
    runtime.publish_staged_reload(staged)
}
