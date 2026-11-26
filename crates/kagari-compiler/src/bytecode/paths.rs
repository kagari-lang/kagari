//! Path-backed Native imports are ordinary calls. Only their link requirements change.
use crate::bytecode::BytecodeLoweringError;
use kagari_bytecode::{module::BytecodeModule, verifier::BytecodeVerificationError};
use kagari_common::cancellation::CancellationToken;
use kagari_mir::verify::VerifiedMirModule;
use std::collections::HashMap;

pub(super) fn attach(
    modules: &mut [BytecodeModule],
    owners: &[VerifiedMirModule],
    cancel: &CancellationToken,
) -> Result<(), BytecodeLoweringError> {
    let cancelled = |_| BytecodeLoweringError::Verification(BytecodeVerificationError::Cancelled);
    let mut paths = HashMap::new();
    for declaration in owners
        .iter()
        .flat_map(|owner| &owner.abi.native_declarations)
    {
        cancel.check().map_err(cancelled)?;
        if let Some(operation) = &declaration.path_operation {
            paths.insert(declaration.declaration, &operation.path);
        }
    }
    if paths.is_empty() {
        return Ok(());
    }
    for module in modules {
        for import in &module.native_imports {
            cancel.check().map_err(cancelled)?;
            if let Some(&path) = paths.get(&import.instance.declaration)
                && !module.path_declarations.contains(path)
            {
                module.path_declarations.push(path.clone());
            }
        }
    }
    Ok(())
}
