//! Representation sizes only; execution and artifact sizes require integration.
use kagari_abi::types::{AbiType, GenericParameterAbi, NominalAbiType};
use kagari_common::identity::{DefinitionPath, table::DefinitionId};

fn main() {
    println!(
        "{{\"owned_binder_bytes\":{},\"scoped_binder_bytes\":{},\"owned_type_bytes\":{},\"scoped_type_bytes\":{},\"owned_nominal_bytes\":{},\"scoped_nominal_bytes\":{}}}",
        size_of::<GenericParameterAbi<DefinitionPath>>(),
        size_of::<GenericParameterAbi<DefinitionId>>(),
        size_of::<AbiType<DefinitionPath>>(),
        size_of::<AbiType<DefinitionId>>(),
        size_of::<NominalAbiType<DefinitionPath>>(),
        size_of::<NominalAbiType<DefinitionId>>(),
    );
}
