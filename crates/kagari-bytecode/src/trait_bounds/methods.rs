//! Validate entry argument mappings and signatures in each interface slot.
use crate::module::{BytecodeModule, CallableTarget};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::types::{ConcreteFunctionIdentity, PublicItem, proofs::ProofCatalog};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::verify::types_in_scope,
    ty::{
        Ty,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};

pub(super) fn valid(
    module: &BytecodeModule,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for table in &module.interface_tables {
        let Some(abi) = module.public_items.iter().find_map(|item| match item {
            PublicItem::InterfaceTable(abi) if abi.declaration == table.declaration => Some(abi),
            _ => None,
        }) else {
            return Ok(false);
        };
        for slot in &table.methods {
            let (identity, body, signature) = match slot.target {
                CallableTarget::Script(target) => {
                    let Some(function) = module.functions.get(target.index()) else {
                        return Ok(false);
                    };
                    let Some(identity) = &function.identity else {
                        return Ok(false);
                    };
                    let semantic = &function.metadata.semantic;
                    let Some(params) = (0..function.metadata.params.len())
                        .map(|index| semantic.params.get(&index).cloned())
                        .collect::<Option<Vec<_>>>()
                    else {
                        return Ok(false);
                    };
                    let Some(result) = semantic.result.clone() else {
                        return Ok(false);
                    };
                    (
                        identity,
                        semantic.generic.as_ref(),
                        Signature { params, result },
                    )
                }
                CallableTarget::Native(target) => {
                    let Some(import) = module.native_imports.get(target.index()) else {
                        return Ok(false);
                    };
                    (
                        &import.instance,
                        import.generic.as_ref(),
                        import.signature.clone(),
                    )
                }
            };
            let Some(name) = slot.method.path.last().map(|part| &part.name) else {
                return Ok(false);
            };
            let Some(method) = abi.methods.iter().find(|method| &method.name == name) else {
                return Ok(false);
            };
            let scope: Vec<_> = abi
                .generic_params
                .iter()
                .chain(&method.generic_params)
                .cloned()
                .collect();
            let parameters = body.map_or(&[][..], |body| body.parameters.as_slice());
            if slot.arguments.len() != parameters.len()
                || !types_in_scope(&slot.arguments, &scope, cancel)
            {
                return Ok(false);
            }
            let mut entry = TypeSubstitution::default();
            for (parameter, argument) in parameters.iter().zip(&slot.arguments) {
                entry.bind(&parameter.owner, parameter.position, argument);
            }
            let mut applied = TypeSubstitution::default();
            if table.arguments.len() != abi.generic_params.len() {
                return Ok(false);
            }
            for (parameter, argument) in abi.generic_params.iter().zip(&table.arguments) {
                applied.bind(&parameter.owner, parameter.position, argument);
            }
            let normalize = |ty: &Ty, substitution: &TypeSubstitution| {
                catalog.normalize(&substitution.apply(ty, cancel)?, cancel)
            };
            let expected = Signature {
                params: method
                    .params
                    .iter()
                    .map(|param| normalize(&param.ty, &applied))
                    .collect::<Result<_, _>>()?,
                result: normalize(&method.return_type, &applied)?,
            };
            let actual = Signature {
                params: signature
                    .params
                    .iter()
                    .map(|ty| normalize(ty, &entry))
                    .collect::<Result<_, _>>()?,
                result: normalize(&signature.result, &entry)?,
            };
            if actual != expected {
                return Ok(false);
            }
            let mut assumptions = applied.apply_bounds(&abi.bounds, cancel)?;
            assumptions.extend(applied.apply_bounds(&method.bounds, cancel)?);
            if let Some(body) = body {
                for bound in entry.apply_bounds(&body.bounds, cancel)? {
                    if !catalog.constraints_hold(
                        &bound.ty,
                        &bound.constraints,
                        &assumptions,
                        cancel,
                    )? {
                        return Ok(false);
                    }
                }
            }
            let target = ConcreteFunctionIdentity {
                declaration: identity.declaration.clone(),
                arguments: identity
                    .arguments
                    .iter()
                    .map(|ty| entry.apply(ty, cancel))
                    .collect::<Result<_, _>>()?,
            };
            if let CallableImplementation::NativeDefault(application) = &method.implementation {
                let CallableTarget::Native(import) = slot.target else {
                    return Ok(false);
                };
                let application = application.apply(&applied, cancel)?;
                let Some(resolved) = catalog.resolve_native_default_in(
                    &application,
                    &scope,
                    &assumptions,
                    cancel,
                )?
                else {
                    return Ok(false);
                };
                if resolved.instance != target
                    || resolved.implementation
                        != CallableImplementation::Native(
                            module.native_imports[import.index()].binding.clone(),
                        )
                    || resolved.signature != actual
                {
                    return Ok(false);
                }
            } else {
                let expected_arguments: Vec<_> = table
                    .arguments
                    .iter()
                    .cloned()
                    .chain(method.generic_params.iter().map(|p| p.as_type()))
                    .collect();
                let mut actual_arguments = target.arguments;
                if matches!(slot.target, CallableTarget::Script(_)) {
                    actual_arguments.extend(slot.arguments.iter().cloned());
                }
                if actual_arguments != expected_arguments {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}
