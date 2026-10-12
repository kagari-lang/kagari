//! Link provider-qualified imports against carried declarations and witnesses.
use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction},
    program::shared,
    verify::ValidatedMirModule,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_contract::{
    callable::witness::OperationWitness,
    native_import::callables::NativeCallableOrigin,
    types::{
        ConcreteFunctionIdentity, PublicItem,
        proofs::{ProofCatalog, implementation::Implementation},
    },
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::ownership::ReceiverOwners,
    ty::{
        GenericParam,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};

pub(super) fn validate(
    caller: &MirModule,
    closure: &[&ValidatedMirModule],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    let tables = closure
        .iter()
        .flat_map(|module| &module.abi.public_items)
        .filter_map(|item| {
            if let PublicItem::InterfaceTable(table) = item {
                (!table.host_bridge).then_some(Implementation::Interface(table.as_ref()))
            } else {
                None
            }
        })
        .collect();
    let mut declarations = Vec::new();
    for module in closure {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        for item in &module.abi.public_items {
            let PublicItem::Trait(record) = item else {
                continue;
            };
            let owner = definitions
                .lookup_root(&module.identity)
                .and_then(|root| {
                    definitions.lookup_child(root, DefinitionKind::Trait, &record.name, 0)
                })
                .ok_or(TypeTransformError::InvalidContract)?;
            declarations.push((owner, record));
        }
        declarations.extend(
            module
                .abi
                .trait_contracts
                .iter()
                .map(|contract| (contract.declaration, &contract.abi)),
        );
    }
    let owners = ReceiverOwners::from_types(closure.iter().flat_map(|module| {
        module.abi.public_items.iter().filter_map(|item| {
            if let PublicItem::Type(ty) = item {
                Some((&module.identity, ty))
            } else {
                None
            }
        })
    }))
    .map_err(|_| TypeTransformError::InvalidContract)?;
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
        definitions,
        cancel,
    )?;
    if !catalog.overrides_valid(&owners, cancel)? {
        return Ok(false);
    }
    for instance in &caller.interface_instances {
        let Some(comparison) = &instance.comparison else {
            continue;
        };
        let Some((table, parameters)) = closure
            .iter()
            .flat_map(|module| &module.abi.public_items)
            .find_map(|item| match item {
                PublicItem::InterfaceTable(table) if table.declaration == instance.declaration => {
                    let parameters = if instance.arguments.iter().any(|ty| !ty.is_concrete()) {
                        table.generic_params.clone()
                    } else {
                        vec![]
                    };
                    table
                        .instantiate(&instance.arguments, &parameters, definitions)
                        .map(|table| (table, parameters))
                }
                _ => None,
            })
        else {
            return Ok(false);
        };
        if !comparison.valid(
            &table.for_type,
            &catalog,
            &parameters,
            &table.bounds,
            cancel,
        )? {
            return Ok(false);
        }
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
                    .filter(|owner| {
                        definitions
                            .resolve(*implementation)
                            .is_ok_and(|view| view.module() == &owner.identity)
                    })
                    .flat_map(|owner| &owner.abi.public_items)
                    .find_map(|item| match item {
                        PublicItem::InterfaceTable(table)
                            if table.declaration == *implementation =>
                        {
                            table.instantiate(
                                arguments,
                                body.map_or(&[], |body| body.parameters.as_slice()),
                                definitions,
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
            .protocol_adapter_valid(Some(&function.instance), catalog.definitions())
        {
            return Ok(false);
        }
        if let Some(required) = &function.semantic.protocol_adapter
            && catalog
                .implicit_callable_signature(
                    required,
                    function
                        .semantic
                        .generic
                        .as_ref()
                        .map_or(&[], |body| body.bounds.as_slice()),
                    cancel,
                )?
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
            if !import.structurally_valid(catalog.definitions()) {
                return Ok(false);
            }
            continue;
        }
        let Some(owner) = closure.iter().find(|owner| {
            definitions
                .resolve(import.instance.declaration)
                .is_ok_and(|view| view.module() == &owner.identity)
        }) else {
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
    for operation in caller.operation_witnesses() {
        let OperationWitness::SharedAdapter(adapter) = operation else {
            continue;
        };
        let Some((body, required, signature)) = adapter.template(definitions) else {
            return Ok(false);
        };
        let Some(target) = closure
            .iter()
            .flat_map(|owner| &owner.functions)
            .find(|function| function.instance == adapter.call.instance)
        else {
            return Ok(false);
        };
        if target.semantic.generic.as_ref() != Some(&body)
            || target.semantic.protocol_adapter.as_ref() != Some(&required)
            || target.semantic.params.len() != signature.params.len()
            || signature
                .params
                .iter()
                .enumerate()
                .any(|(index, ty)| target.semantic.params.get(&index) != Some(ty))
            || target.semantic.result.as_ref() != Some(&signature.result)
        {
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
                .find(|owner| {
                    definitions
                        .resolve(callable.instance.declaration)
                        .is_ok_and(|view| view.module() == &owner.identity)
                })
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
    let definitions = catalog.definitions();
    let base = module
        .abi
        .public_items
        .iter()
        .filter_map(|item| match item {
            PublicItem::InterfaceTable(table) if table.generic_params.is_empty() => {
                Some(ConcreteFunctionIdentity {
                    declaration: table.declaration,
                    arguments: vec![],
                })
            }
            _ => None,
        });
    for instance in base.chain(
        module
            .interface_instances
            .iter()
            .map(|instance| instance.identity()),
    ) {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if definitions
            .resolve(instance.declaration)
            .map_err(|_| TypeTransformError::InvalidContract)?
            .module()
            != &module.identity
        {
            continue;
        }
        let Some(template) = module.abi.public_items.iter().find_map(|item| match item {
            PublicItem::InterfaceTable(table) if table.declaration == instance.declaration => {
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
        let Some(table) =
            template.instantiate(&instance.arguments, &template.generic_params, definitions)
        else {
            return Ok(false);
        };
        for method in &table.methods {
            if !method.generic_params.is_empty() {
                continue;
            }
            let declaration = definitions
                .lookup_child(
                    instance.declaration,
                    DefinitionKind::Method,
                    &method.name,
                    0,
                )
                .ok_or(TypeTransformError::InvalidContract)?;
            let signature = Signature {
                params: method
                    .params
                    .iter()
                    .map(|param| catalog.normalize(&param.ty, cancel))
                    .collect::<Result<_, _>>()?,
                result: catalog.normalize(&method.return_type, cancel)?,
            };
            let mut assumptions = table.bounds.clone();
            assumptions.extend(method.bounds.clone());
            let (target_instance, binding) = match &method.implementation {
                CallableImplementation::Native(binding) => (
                    ConcreteFunctionIdentity {
                        declaration,
                        arguments: instance.arguments.clone(),
                    },
                    *binding,
                ),
                CallableImplementation::NativeDefault(application) => {
                    let Some(resolved) = catalog.resolve_native_default_in(
                        application,
                        &template.generic_params,
                        &assumptions,
                        cancel,
                    )?
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
            let mut found = false;
            for target in &module.native_targets {
                if target.instance.declaration != target_instance.declaration
                    || target.binding != binding
                    || target.host.is_some()
                {
                    continue;
                }
                let Some(body) = &target.generic else {
                    found |= target.instance == target_instance && target.signature == signature;
                    continue;
                };
                if body.parameters.len() != target_instance.arguments.len()
                    || target.instance.arguments
                        != body
                            .parameters
                            .iter()
                            .map(GenericParam::as_type)
                            .collect::<Vec<_>>()
                {
                    continue;
                }
                let mut substitution = TypeSubstitution::default();
                for (parameter, argument) in body.parameters.iter().zip(&target_instance.arguments)
                {
                    substitution.bind(&parameter.owner, parameter.position, argument);
                }
                let actual = Signature {
                    params: target
                        .signature
                        .params
                        .iter()
                        .map(|ty| catalog.normalize(&substitution.apply(ty, cancel)?, cancel))
                        .collect::<Result<_, _>>()?,
                    result: catalog.normalize(
                        &substitution.apply(&target.signature.result, cancel)?,
                        cancel,
                    )?,
                };
                if actual != signature {
                    continue;
                }
                let mut valid = true;
                for bound in
                    substitution.apply_bounds(&body.bounds, catalog.definitions(), cancel)?
                {
                    valid &= catalog.constraints_hold(
                        &bound.ty,
                        &bound.constraints,
                        &assumptions,
                        cancel,
                    )?;
                }
                found |= valid;
            }
            if !found {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
