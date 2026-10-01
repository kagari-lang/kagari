//! Compare executable method signatures after canonical substitution and projection resolution.
use crate::module::{BytecodeModule, CallableTarget};
use kagari_abi::types::{
    AbiType, PublicAbiItem,
    proofs::ProofCatalog,
    substitution::{TypeSubstitution, TypeTransformError},
};
use kagari_common::cancellation::CancellationToken;

pub(super) fn valid(
    module: &BytecodeModule,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for table in &module.interface_tables {
        let Some(abi) = module.public_items.iter().find_map(|item| match item {
            PublicAbiItem::InterfaceTable(abi) if abi.declaration == table.declaration => Some(abi),
            _ => None,
        }) else {
            return Ok(false);
        };
        for slot in &table.methods {
            let identity = match slot.target {
                CallableTarget::Script(target) => module
                    .functions
                    .get(target.index())
                    .and_then(|function| function.identity.as_ref()),
                CallableTarget::Native(target) => module
                    .native_imports
                    .get(target.index())
                    .map(|import| &import.instance),
            };
            let Some(identity) = identity else {
                return Ok(false);
            };
            let Some(name) = slot.method.path.last().map(|part| &part.name) else {
                return Ok(false);
            };
            let Some(method) = abi.methods.iter().find(|method| &method.name == name) else {
                return Ok(false);
            };
            if identity.arguments.len() != abi.generic_params.len() + method.generic_params.len() {
                return Ok(false);
            }
            let mut substitution = TypeSubstitution::default();
            for (parameter, argument) in abi
                .generic_params
                .iter()
                .chain(&method.generic_params)
                .zip(&identity.arguments)
            {
                substitution.bind(&parameter.owner, parameter.position, argument);
            }
            let normalize =
                |ty: &AbiType| catalog.normalize(&substitution.apply(ty, cancel)?, cancel);
            let params = method
                .params
                .iter()
                .map(|param| normalize(&param.ty))
                .collect::<Result<Vec<_>, _>>()?;
            let result = normalize(&method.return_type)?;
            match slot.target {
                CallableTarget::Script(target) => {
                    let function = &module.functions[target.index()];
                    if params
                        .iter()
                        .map(AbiType::representation)
                        .collect::<Vec<_>>()
                        != function.metadata.params
                        || result.representation() != function.metadata.return_type
                        || params.iter().enumerate().any(|(index, ty)| {
                            function.metadata.semantic.params.get(&index) != Some(ty)
                        })
                        || function.metadata.semantic.result.as_ref() != Some(&result)
                    {
                        return Ok(false);
                    }
                }
                CallableTarget::Native(target) => {
                    let signature = &module.native_imports[target.index()].signature;
                    if signature.params != params || signature.result != result {
                        return Ok(false);
                    }
                }
            }
        }
    }
    Ok(true)
}
