//! Recipient-local raw unit fixtures; detached consumers use the external helper.
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
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
    let verified = VerifiedProgram::new(bytecode, runtime.definition_context())?;
    runtime.load_verified_program(name, verified)
}

pub(crate) fn reload_bytecode(
    runtime: &mut Runtime,
    active: &InstalledProgram,
    name: Option<String>,
    bytecode: DefinitionMetadata<BytecodeProgram>,
) -> Result<(), ReloadValidationError> {
    let staged = runtime.stage_reload_program(active, name, bytecode)?;
    runtime.publish_staged_reload(staged)
}
