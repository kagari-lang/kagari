//! Link provider-qualified imports against carried declarations and witnesses.
use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction},
    program::shared,
    verify::VerifiedMirModule,
};
use kagari_abi::{
    callable::CallableImplementation,
    native_import::{NativeSignature, callables::NativeCallableOrigin},
    types::{
        ConcreteFunctionIdentity, GenericParameterAbi, PublicAbiItem,
        proofs::{ProofCatalog, implementation::Implementation},
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
};

pub(super) fn validate(
    caller: &MirModule,
    closure: &[&VerifiedMirModule],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    let tables = closure
        .iter()
        .flat_map(|module| &module.abi.public_items)
        .filter_map(|item| {
            if let PublicAbiItem::InterfaceTable(table) = item {
                (!table.host_bridge).then_some(Implementation::Interface(table.as_ref()))
            } else {
                None
            }
        })
        .collect();
    let declarations = closure.iter().flat_map(|module| {
        let public = module.abi.public_items.iter().filter_map(|item| {
            let PublicAbiItem::Trait(record) = item else {
                return None;
            };
            Some((
                DefinitionId {
                    module: module.identity.clone(),
                    path: vec![DefinitionPathSegment {
                        kind: DefinitionKind::Trait,
                        name: record.name.clone(),
                        occurrence: 0,
                    }],
                },
                record,
            ))
        });
        public.chain(
            module
                .abi
                .trait_contracts
                .iter()
                .map(|contract| (contract.declaration.clone(), &contract.abi)),
        )
    });
    let catalog = ProofCatalog::new(
        tables,
        closure
            .iter()
            .flat_map(|module| &module.host_types)
            .collect(),
        closure.iter().flat_map(|module| &module.enumerations),
        declarations,
        closure
            .iter()
            .flat_map(|module| &module.abi.native_declarations),
        cancel,
    )?;
    if !catalog.overrides_valid(cancel)? {
        return Ok(false);
    }
    for function in &caller.functions {
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            if let Instruction::MakeInterface {
                implementation,
                arguments,
                ..
            } = instruction
            {
                let body = function.semantic.generic.as_ref();
                let Some(table) = closure
                    .iter()
                    .filter(|owner| owner.identity == implementation.module)
                    .flat_map(|owner| &owner.abi.public_items)
                    .find_map(|item| match item {
                        PublicAbiItem::InterfaceTable(table)
                            if table.declaration == *implementation =>
                        {
                            table.instantiate_in(
                                arguments,
                                body.map_or(&[], |body| body.parameters.as_slice()),
                            )
                        }
                        _ => None,
                    })
                else {
                    return Ok(false);
                };
                for bound in &table.bounds {
                    if !catalog.constraints_hold(
                        &bound.ty,
                        &bound.constraints,
                        body.map_or(&[], |body| body.bounds.as_slice()),
                        cancel,
                    )? {
                        return Ok(false);
                    }
                }
            }
            if let Instruction::Call {
                callee: CallTarget::InterfaceMethod(call),
                ..
            } = instruction
            {
                let Some(contract) = catalog.trait_contract(&call.interface.declaration) else {
                    return Ok(false);
                };
                let body = function.semantic.generic.as_ref();
                if !call.check(
                    contract,
                    &catalog,
                    body.map_or(&[], |body| body.parameters.as_slice()),
                    body.map_or(&[], |body| body.bounds.as_slice()),
                    cancel,
                )? {
                    return Ok(false);
                }
            }
        }
        if !function
            .semantic
            .protocol_adapter_valid(Some(&function.instance))
        {
            return Ok(false);
        }
        if let Some(required) = &function.semantic.protocol_adapter
            && catalog
                .implicit_callable_signature(required, cancel)?
                .is_none()
        {
            return Ok(false);
        }
    }
    if !native_slots_valid(caller, &catalog, cancel)? {
        return Ok(false);
    }
    if !shared::valid(caller, closure, &catalog, cancel)? {
        return Ok(false);
    }
    if caller
        .abi
        .native_declarations
        .iter()
        .flat_map(|declaration| &declaration.callable_requirements)
        .any(|required| !catalog.callable_requirement_valid(required))
    {
        return Ok(false);
    }
    for import in caller.native_applications() {
        if import.host.is_some() {
            if !import.structurally_valid() {
                return Ok(false);
            }
            continue;
        }
        let Some(owner) = closure
            .iter()
            .find(|owner| owner.identity == import.instance.declaration.module)
        else {
            return Ok(false);
        };
        let Some(declaration) = owner
            .abi
            .native_declarations
            .iter()
            .find(|declaration| declaration.declaration == import.instance.declaration)
        else {
            return Ok(false);
        };
        if !import.matches_declaration(declaration, &catalog, cancel)? {
            return Ok(false);
        }
    }
    for callable in caller.selected_callables() {
        if let CallableImplementation::Native(binding) = &callable.implementation
            && !closure
                .iter()
                .flat_map(|owner| &owner.native_targets)
                .any(|target| {
                    target.instance == callable.instance
                        && &target.binding == binding
                        && target.signature == callable.signature
                        && target.host.is_none()
                })
        {
            return Ok(false);
        }
        if callable.implementation == CallableImplementation::Script {
            let Some(target) = closure
                .iter()
                .find(|owner| owner.identity == callable.instance.declaration.module)
                .and_then(|owner| {
                    owner
                        .functions
                        .iter()
                        .find(|function| function.instance == callable.instance)
                })
            else {
                return Ok(false);
            };
            let marker_valid = match callable.origin {
                NativeCallableOrigin::Implementation => target.semantic.protocol_adapter.is_none(),
                NativeCallableOrigin::ProtocolAdapter => {
                    target.semantic.protocol_adapter.as_ref() == Some(&callable.requirement)
                }
            };
            if !marker_valid
                || target.params.len() != callable.signature.params.len()
                || target.semantic.params.len() != callable.signature.params.len()
                || callable
                    .signature
                    .params
                    .iter()
                    .enumerate()
                    .any(|(index, ty)| target.semantic.params.get(&index) != Some(ty))
                || target.semantic.result.as_ref() != Some(&callable.signature.result)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn native_slots_valid(
    module: &MirModule,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    let base = module
        .abi
        .public_items
        .iter()
        .filter_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) if table.generic_params.is_empty() => {
                Some(ConcreteFunctionIdentity {
                    declaration: table.declaration.clone(),
                    arguments: vec![],
                })
            }
            _ => None,
        });
    for instance in base.chain(module.interface_instances.iter().cloned()) {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if instance.declaration.module != module.identity {
            continue;
        }
        let Some(template) = module.abi.public_items.iter().find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) if table.declaration == instance.declaration => {
                Some(table)
            }
            _ => None,
        }) else {
            return Ok(false);
        };
        if template.host_bridge
            || instance.arguments.is_empty() && !template.generic_params.is_empty()
        {
            continue;
        }
        let Some(table) = template.instantiate_in(&instance.arguments, &template.generic_params)
        else {
            return Ok(false);
        };
        for method in &table.methods {
            if !method.generic_params.is_empty() {
                continue;
            }
            let mut declaration = instance.declaration.clone();
            declaration.path.push(DefinitionPathSegment {
                kind: DefinitionKind::Method,
                name: method.name.clone(),
                occurrence: 0,
            });
            let mut signature = NativeSignature {
                params: method
                    .params
                    .iter()
                    .map(|param| catalog.normalize(&param.ty, cancel))
                    .collect::<Result<_, _>>()?,
                result: catalog.normalize(&method.return_type, cancel)?,
            };
            let (target_instance, binding) = match &method.implementation {
                CallableImplementation::Native(binding) => {
                    let arguments = if instance.arguments.iter().any(|ty| !ty.is_concrete()) {
                        let arguments: Vec<_> = template
                            .generic_params
                            .iter()
                            .enumerate()
                            .map(|(position, _)| {
                                GenericParameterAbi {
                                    owner: declaration.clone(),
                                    position,
                                }
                                .as_type()
                            })
                            .collect();
                        let mut substitution = TypeSubstitution::default();
                        for (parameter, argument) in template.generic_params.iter().zip(&arguments)
                        {
                            substitution.bind(&parameter.owner, parameter.position, argument);
                        }
                        signature.params = signature
                            .params
                            .iter()
                            .map(|ty| substitution.apply(ty, cancel))
                            .collect::<Result<_, _>>()?;
                        signature.result = substitution.apply(&signature.result, cancel)?;
                        arguments
                    } else {
                        instance.arguments.clone()
                    };
                    (
                        ConcreteFunctionIdentity {
                            declaration,
                            arguments,
                        },
                        binding.clone(),
                    )
                }
                CallableImplementation::NativeDefault(application) => {
                    let Some(resolved) = catalog.resolve_native_default(application, cancel)?
                    else {
                        return Ok(false);
                    };
                    let CallableImplementation::Native(binding) = resolved.implementation else {
                        return Ok(false);
                    };
                    if resolved.signature != signature {
                        return Ok(false);
                    }
                    (resolved.instance, binding)
                }
                _ => continue,
            };
            if !module.native_targets.iter().any(|target| {
                target.instance == target_instance
                    && target.binding == binding
                    && target.signature == signature
                    && target.host.is_none()
            }) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
