use crate::{
    native_import::{NativeImport, callables::normalize_requirement},
    types::proofs::ProofCatalog,
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    callable::CallableImplementation,
    declaration::NativeDeclaration,
    ty::substitution::{TypeSubstitution, TypeTransformError},
};

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
    match (&declaration.concrete_result, &import.result_adapter) {
        (None, None) => {}
        (Some(receiver), Some(adapter))
            if normalize(receiver)? == adapter.receiver
                && catalog.native_result_matches(
                    adapter,
                    &import.signature.result,
                    &bounds,
                    cancel,
                )? => {}
        _ => return Ok(false),
    }
    for bound in &bounds {
        if !catalog.constraints_hold(
            &bound.ty,
            &bound.constraints,
            import
                .generic
                .as_ref()
                .map_or(&[], |body| body.bounds.as_slice()),
            cancel,
        )? {
            return Ok(false);
        }
    }
    if declaration.callable_requirements.len() != import.callables.len() {
        return Ok(false);
    }
    for (required, selected) in declaration
        .callable_requirements
        .iter()
        .zip(&import.callables)
    {
        let applied = required.apply(&substitution, cancel)?;
        let required = normalize_requirement(&applied, catalog, cancel)?;
        if selected.requirement() != &required
            || !selected.valid(
                catalog,
                import
                    .generic
                    .as_ref()
                    .map_or(&[], |body| body.parameters.as_slice()),
                import
                    .generic
                    .as_ref()
                    .map_or(&[], |body| body.bounds.as_slice()),
                cancel,
            )?
        {
            return Ok(false);
        }
    }
    Ok(true)
}
