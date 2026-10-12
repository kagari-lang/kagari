//! Representation sizes only; execution and artifact sizes require integration.
use kagari_types::ty::{GenericParam, NominalTy, Ty};

fn main() {
    println!(
        "{{\"scoped_binder_bytes\":{},\"scoped_type_bytes\":{},\"scoped_nominal_bytes\":{}}}",
        size_of::<GenericParam>(),
        size_of::<Ty>(),
        size_of::<NominalTy>(),
    );
}
