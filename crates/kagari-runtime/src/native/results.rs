//! Construct native readonly results through their checked concrete interface table.
use crate::{LoadedModule, Runtime, RuntimeError, value::Value};
use kagari_abi::{
    native_import::{EngineNativeImport, NativeWitnessImplementation},
    types::AbiType,
};
pub(super) fn readonly_list(
    runtime: &Runtime,
    owner: &LoadedModule,
    contract: &EngineNativeImport,
    storage: Value,
) -> Result<Value, RuntimeError> {
    let invalid = || RuntimeError::module_validation("native readonly result contract mismatch");
    let AbiType::Trait(interface) = &contract.signature.result else {
        return Err(invalid());
    };
    let factory = contract
        .witnesses
        .iter()
        .find(|w| w.interface == *interface)
        .ok_or_else(invalid)?;
    let NativeWitnessImplementation::Table(target) = &factory.implementation else {
        return Err(invalid());
    };
    let implementation = owner
        .members()
        .find(|m| m.bytecode.identity == target.declaration.module)
        .ok_or_else(invalid)?;
    runtime.validate_loaded_module(&implementation)?;
    let index = implementation
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            table.declaration == target.declaration && table.arguments == target.arguments
        })
        .ok_or_else(invalid)?;
    let value = runtime.make_interface(&implementation, index, storage)?;
    if !runtime.matches_interface_method_abi(&value, &contract.signature.result, owner) {
        return Err(invalid());
    }
    Ok(value)
}
