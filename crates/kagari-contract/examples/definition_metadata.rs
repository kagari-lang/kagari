//! Representation sizes only; execution and artifact sizes require integration.
use kagari_common::identity::{DefinitionPath, table::DefinitionId};
use kagari_contract::types::{GenericParam, NominalTy, Ty};

fn main() {
    println!(
        "{{\"owned_binder_bytes\":{},\"scoped_binder_bytes\":{},\"owned_type_bytes\":{},\"scoped_type_bytes\":{},\"owned_nominal_bytes\":{},\"scoped_nominal_bytes\":{}}}",
        size_of::<GenericParam<DefinitionPath>>(),
        size_of::<GenericParam<DefinitionId>>(),
        size_of::<Ty<DefinitionPath>>(),
        size_of::<Ty<DefinitionId>>(),
        size_of::<NominalTy<DefinitionPath>>(),
        size_of::<NominalTy<DefinitionId>>(),
    );
}
