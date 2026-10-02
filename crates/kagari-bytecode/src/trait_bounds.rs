//! Recheck associated outputs and host trait bounds against the dependency closure.
mod applications;
mod associated;
mod callables;
mod methods;
pub(crate) mod shared;
pub mod views;

use crate::{
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeModule,
    program::BytecodeProgram,
    trait_bounds::associated::{associated_bounds_match, host_bounds_match},
    verifier::BytecodeVerificationError,
};
use kagari_abi::{
    callable::interface::InterfaceCallContract,
    language::Protocol,
    types::{
        self as abi, AbiType, GenericBoundAbi, GenericParameterAbi, NominalAbiType, PublicAbiItem,
        TraitAbi, inheritance as trait_inheritance,
        proofs::{ProofCatalog, host_application, implementation::Implementation},
        substitution::TypeTransformError,
        verify,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
};

fn contract<'a>(id: &DefinitionId, closure: &[&'a BytecodeModule]) -> Option<&'a TraitAbi> {
    let owner = closure.iter().find(|module| module.identity == id.module)?;
    abi::trait_contract(
        &owner.identity,
        &owner.public_items,
        &owner.trait_contracts,
        id,
    )
}

/// Read the bounded applied parent closure from portable trait contracts.
pub fn interface_ancestors(
    interface: &NominalAbiType,
    receiver: &AbiType,
    closure: &[&BytecodeModule],
) -> Option<Vec<NominalAbiType>> {
    trait_inheritance::trait_closure(interface, receiver, &CancellationToken::default(), &|id| {
        contract(id, closure)
    })
    .ok()
}

/// Checked dynamic surfaces; concrete implementation proofs use raw ancestry.
pub fn interface_views(
    interface: &NominalAbiType,
    receiver: &AbiType,
    closure: &[&BytecodeModule],
) -> Option<Vec<NominalAbiType>> {
    trait_inheritance::interface_views(interface, receiver, &CancellationToken::default(), &|id| {
        contract(id, closure)
    })
    .ok()
}

fn executable_interface(
    scope: &[GenericParameterAbi],
    applied: &NominalAbiType,
    receiver: &AbiType,
    closure: &[&BytecodeModule],
) -> bool {
    let Some(preserved) = interface_ancestors(applied, &AbiType::Trait(applied.clone()), closure)
    else {
        return false;
    };
    if interface_ancestors(applied, receiver, closure).as_ref() != Some(&preserved) {
        return false;
    }
    let Some(views) = interface_views(applied, &AbiType::Trait(applied.clone()), closure) else {
        return false;
    };
    for view in views {
        if Protocol::from_id(&view.declaration).is_some_and(|kind| !kind.dynamic()) {
            return false;
        }
        let Some(record) = contract(&view.declaration, closure) else {
            return false;
        };
        if !record.associated_consts.is_empty()
            || record
                .associated_types
                .iter()
                .any(|member| !member.generic_params.is_empty())
            || view.associated_types.len() != record.associated_types.len()
            || record
                .associated_types
                .iter()
                .any(|member| !view.associated_types.contains_key(&member.declaration))
        {
            return false;
        }
        let Some(owner) = closure
            .iter()
            .find(|module| module.identity == view.declaration.module)
        else {
            return false;
        };
        for (slot, method) in record.methods.iter().enumerate() {
            let call = InterfaceCallContract {
                receiver: None,
                operations: vec![],
                interface: view.clone(),
                method_slot: slot as u32,
                arguments: method
                    .generic_params
                    .iter()
                    .map(GenericParameterAbi::as_type)
                    .collect(),
            };
            if !call
                .signature_in(
                    &owner.identity,
                    &owner.public_items,
                    &owner.trait_contracts,
                    &Default::default(),
                )
                .is_ok_and(|signature| {
                    signature.types_valid(
                        &scope
                            .iter()
                            .chain(&method.generic_params)
                            .cloned()
                            .collect::<Vec<_>>(),
                        &Default::default(),
                    )
                })
            {
                return false;
            }
        }
    }
    true
}

fn declarations(module: &BytecodeModule) -> impl Iterator<Item = (DefinitionId, &TraitAbi)> {
    let public = module.public_items.iter().filter_map(|item| {
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
            .trait_contracts
            .iter()
            .map(|record| (record.declaration.clone(), &record.abi)),
    )
}

pub(super) fn verify_trait_bounds(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    program: Option<&BytecodeProgram>,
) -> Result<(), BytecodeVerificationError> {
    match linked_bounds_match(module, closure, program) {
        Ok(true) => Ok(()),
        Err(LinkedValidationError::InterfaceTable) => {
            Err(BytecodeVerificationError::InvalidInterfaceTable)
        }
        _ => Err(BytecodeVerificationError::InvalidHostInterface(
            "trait output or host bound has no unique valid implementation".into(),
        )),
    }
}

enum LinkedValidationError {
    InterfaceTable,
    Contract,
}
impl From<TypeTransformError> for LinkedValidationError {
    fn from(_: TypeTransformError) -> Self {
        Self::Contract
    }
}

fn linked_bounds_match(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    program: Option<&BytecodeProgram>,
) -> Result<bool, LinkedValidationError> {
    let cancel = CancellationToken::default();
    applications::validate(module, closure, &cancel)?;
    let tables: Vec<_> = closure
        .iter()
        .flat_map(|dependency| &dependency.public_items)
        .filter_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) if !table.host_bridge => Some(table.as_ref()),
            _ => None,
        })
        .collect();
    let catalog = ProofCatalog::new(
        tables
            .iter()
            .copied()
            .map(Implementation::Interface)
            .collect(),
        closure
            .iter()
            .flat_map(|module| &module.host_interface.types)
            .collect(),
        closure.iter().flat_map(|module| &module.enumerations),
        closure.iter().flat_map(|module| declarations(module)),
        closure
            .iter()
            .flat_map(|module| &module.native_declarations),
        &cancel,
    )?;
    if !views::valid(module, closure, &cancel).map_err(|_| LinkedValidationError::InterfaceTable)? {
        return Err(LinkedValidationError::InterfaceTable);
    }
    if !methods::valid(module, &catalog, &cancel)
        .map_err(|_| LinkedValidationError::InterfaceTable)?
    {
        return Err(LinkedValidationError::InterfaceTable);
    }
    if !shared::valid(module, closure, program, &catalog, &cancel)? {
        return Ok(false);
    }
    if !catalog.overrides_valid(&cancel)? || !instruction_contracts_match(module, closure, program)
    {
        return Ok(false);
    }
    for function in &module.functions {
        for instruction in &function.instructions {
            if let BytecodeInstruction::MakeInterface {
                module: owner,
                implementation,
                arguments,
                ..
            } = instruction
            {
                let Some(target) = program
                    .and_then(|program| program.modules.get(owner.index()))
                    .or_else(|| (program.is_none() && owner.index() == 0).then_some(module))
                else {
                    return Ok(false);
                };
                let Some(linked) = target.interface_tables.get(implementation.index()) else {
                    return Ok(false);
                };
                let body = function.metadata.semantic.generic.as_ref();
                let Some(table) = target.public_items.iter().find_map(|item| match item {
                    PublicAbiItem::InterfaceTable(table)
                        if table.declaration == linked.declaration =>
                    {
                        table.instantiate_in(
                            arguments,
                            body.map_or(&[], |body| body.parameters.as_slice()),
                        )
                    }
                    _ => None,
                }) else {
                    return Ok(false);
                };
                for bound in &table.bounds {
                    if !catalog.constraints_hold(
                        &bound.ty,
                        &bound.constraints,
                        body.map_or(&[], |body| body.bounds.as_slice()),
                        &cancel,
                    )? {
                        return Ok(false);
                    }
                }
            }
            if let BytecodeInstruction::Call {
                callee: CallTarget::InterfaceMethod { contract: call, .. },
                ..
            } = instruction
            {
                let Some(declaration) = contract(&call.interface.declaration, closure) else {
                    return Ok(false);
                };
                let body = function.metadata.semantic.generic.as_ref();
                if !call.check(
                    declaration,
                    &catalog,
                    body.map_or(&[], |body| body.parameters.as_slice()),
                    body.map_or(&[], |body| body.bounds.as_slice()),
                    &cancel,
                )? {
                    return Ok(false);
                }
                for operation in &call.operations {
                    if !callables::witness_valid(operation, closure) {
                        return Ok(false);
                    }
                }
            }
        }
        if !function
            .metadata
            .semantic
            .protocol_adapter_valid(function.identity.as_ref())
        {
            return Ok(false);
        }
        if let Some(required) = &function.metadata.semantic.protocol_adapter
            && catalog
                .implicit_callable_signature(required, &cancel)?
                .is_none()
        {
            return Ok(false);
        }
    }
    if module
        .native_declarations
        .iter()
        .flat_map(|declaration| &declaration.callable_requirements)
        .any(|required| !catalog.callable_requirement_valid(required))
    {
        return Ok(false);
    }
    for import in &module.native_imports {
        if let Some(host) = &import.host {
            if !import.structurally_valid() || !module.host_interface.functions.contains(host) {
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
            .native_declarations
            .iter()
            .find(|declaration| declaration.declaration == import.instance.declaration)
        else {
            return Ok(false);
        };
        if !import.matches_declaration(declaration, &catalog, &cancel)? {
            return Ok(false);
        }
        if import
            .callables
            .iter()
            .any(|call| !callables::witness_valid(call, closure))
        {
            return Ok(false);
        }
    }
    // Validate unused declaration graphs and constructor references too.
    for (id, record) in closure.iter().flat_map(|module| declarations(module)) {
        let applied = NominalAbiType {
            declaration: id.clone(),
            arguments: record
                .generic_params
                .iter()
                .map(GenericParameterAbi::as_type)
                .collect(),
            associated_types: Default::default(),
        };
        catalog.ancestry(&applied, &AbiType::SelfType(id), &cancel)?;
    }
    for table in tables {
        let AbiType::Trait(applied) = &table.trait_type else {
            return Ok(false);
        };
        if !parents_proven(
            applied,
            &table.for_type,
            &table.bounds,
            &catalog,
            closure,
            &cancel,
        )? {
            return Ok(false);
        }
    }
    for host in closure
        .iter()
        .flat_map(|module| &module.host_interface.types)
    {
        for implementation in &host.trait_implementations {
            if !parents_proven(
                &host_application(implementation),
                &AbiType::Host(host.id.clone()),
                &[],
                &catalog,
                closure,
                &cancel,
            )? {
                return Ok(false);
            }
        }
    }
    for item in &module.public_items {
        let PublicAbiItem::InterfaceTable(table) = item else {
            continue;
        };
        let AbiType::Trait(interface) = &table.trait_type else {
            return Ok(false);
        };
        let Some(record) = contract(&interface.declaration, closure) else {
            return Ok(false);
        };
        if interface.associated_types.len()
            != record
                .associated_types
                .iter()
                .filter(|member| member.generic_params.is_empty())
                .count()
            || !verify::interface_constants_match(table, record)
            || !verify::interface_families_match(table, record, &cancel)
            || !verify::interface_methods_match(
                table,
                record,
                &|ty| catalog.normalize(ty, &cancel),
                &cancel,
            )
        {
            return Ok(false);
        }
        if !table.host_bridge
            && matches!(table.for_type, AbiType::Host(_))
            && catalog.implementation_count(interface, &table.for_type, &table.bounds, &cancel)?
                != 1
        {
            return Ok(false);
        }
        if !associated_bounds_match(table, interface, record, &catalog, closure, &cancel)? {
            return Ok(false);
        }
    }
    for linked in &module.interface_tables {
        if linked.arguments.is_empty() {
            continue;
        }
        let Some(table) = module.public_items.iter().find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                table.instantiate_in(&linked.arguments, &table.generic_params)
            }
            _ => None,
        }) else {
            return Ok(false);
        };
        let AbiType::Trait(interface) = &table.trait_type else {
            return Ok(false);
        };
        if catalog.implementation_count(interface, &table.for_type, &table.bounds, &cancel)? != 1 {
            return Ok(false);
        }
    }
    Ok(host_bounds_match(module, closure, &catalog, &cancel)?)
}

fn parents_proven(
    interface: &NominalAbiType,
    receiver: &AbiType,
    bounds: &[GenericBoundAbi],
    catalog: &ProofCatalog<'_>,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    // Offline host declarations may advertise traits outside this program.
    if matches!(receiver, AbiType::Host(_))
        && Protocol::from_id(&interface.declaration).is_none()
        && contract(&interface.declaration, closure).is_none()
    {
        return Ok(true);
    }
    let bounds = catalog.expand_bounds(bounds, cancel)?;
    for parent in catalog
        .ancestry(interface, receiver, cancel)?
        .into_iter()
        .skip(1)
    {
        if !catalog.holds(&parent, receiver, &bounds, cancel)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn instruction_contracts_match(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    program: Option<&BytecodeProgram>,
) -> bool {
    for (function, instruction) in module.functions.iter().flat_map(|function| {
        function
            .instructions
            .iter()
            .map(move |instruction| (function, instruction))
    }) {
        if let BytecodeInstruction::UpcastInterface { source, target, .. } = instruction {
            let Some(parents) = interface_views(source, &AbiType::Trait(source.clone()), closure)
            else {
                return false;
            };
            if !parents.contains(target) {
                return false;
            }
        }
        if let BytecodeInstruction::MakeInterface {
            module: owner,
            implementation,
            arguments,
            ..
        } = instruction
        {
            let target = program
                .and_then(|program| program.modules.get(owner.index()))
                .or_else(|| (program.is_none() && owner.index() == 0).then_some(module));
            let Some(target) = target else {
                return false;
            };
            let Some(linked) = target.interface_tables.get(implementation.index()) else {
                return false;
            };
            let Some(table) = target.public_items.iter().find_map(|item| match item {
                PublicAbiItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                    table.instantiate_in(
                        arguments,
                        function
                            .metadata
                            .semantic
                            .generic
                            .as_ref()
                            .map_or(&[][..], |body| body.parameters.as_slice()),
                    )
                }
                _ => None,
            }) else {
                return false;
            };
            let AbiType::Trait(applied) = &table.trait_type else {
                return false;
            };
            if !executable_interface(
                function
                    .metadata
                    .semantic
                    .generic
                    .as_ref()
                    .map_or(&[][..], |body| body.parameters.as_slice()),
                applied,
                &table.for_type,
                closure,
            ) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests;
