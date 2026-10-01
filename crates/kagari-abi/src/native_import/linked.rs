use crate::{
    callable::CallableImplementation,
    native_import::NativeImport,
    types::{
        NativeDeclaration,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::cancellation::CancellationToken;

pub(super) fn matches_declaration(
    import: &NativeImport,
    declaration: &NativeDeclaration,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    let function = &declaration.function;
    if import.instance.declaration != declaration.declaration
        || function.implementation != CallableImplementation::Native(import.binding.clone())
        || import.instance.arguments.len() != function.generic_params.len()
        || !import.structurally_valid()
    {
        return Ok(false);
    }
    let mut substitution = TypeSubstitution::default();
    for (parameter, argument) in function
        .generic_params
        .iter()
        .zip(&import.instance.arguments)
    {
        substitution.bind(&parameter.owner, parameter.position, argument);
    }
    let normalize = |ty| catalog.normalize(&substitution.apply(ty, cancel)?, cancel);
    if function.params.len() != import.signature.params.len()
        || normalize(&function.return_type)? != import.signature.result
    {
        return Ok(false);
    }
    for (parameter, actual) in function.params.iter().zip(&import.signature.params) {
        if normalize(&parameter.ty)? != *actual {
            return Ok(false);
        }
    }
    let bounds = substitution.apply_bounds(&function.bounds, cancel)?;
    if bounds != import.requirements {
        return Ok(false);
    }
    for bound in &bounds {
        if !catalog.constraints_hold(&bound.ty, &bound.constraints, &[], cancel)? {
            return Ok(false);
        }
    }
    Ok(true)
}
