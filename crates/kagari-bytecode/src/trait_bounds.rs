//! Recheck associated outputs and host trait bounds against the dependency closure.
mod applications;
mod associated;
mod callables;
mod methods;
mod native;
pub(crate) mod preparation;
pub(crate) mod shared;
pub mod views;

use crate::{
    instruction::{
        BinaryOp, BytecodeInstruction, CallTarget, ConstructionTarget as BytecodeConstruction,
        ScalarOperand,
    },
    module::BytecodeModule,
    program::{BytecodeProgram, native::NativeApplicationWitness},
    trait_bounds::{
        associated::associated_bounds_match,
        preparation::{VerificationResources, VerificationScope},
    },
    verifier::BytecodeVerificationError,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    callable::interface::dynamic_interface_valid,
    representation::builtin_representation,
    standard::RuntimePrimitive,
    types as abi,
    types::{PublicItem, proofs::ProofCatalog, verify},
};
use kagari_types::{
    declaration::TraitDef,
    language::Protocol,
    ty::{
        GenericBound, GenericParam, NominalTy, Ty, inheritance as trait_inheritance,
        substitution::TypeTransformError,
    },
};

use std::borrow::Cow;

fn contract<'a>(
    id: &DefinitionId,
    closure: &[&'a BytecodeModule],
    definitions: &DefinitionTable,
) -> Option<&'a TraitDef> {
    let identity = definitions.resolve(*id).ok()?;
    let owner = closure
        .iter()
        .find(|module| &module.identity == identity.module())?;
    abi::trait_contract(
        &owner.identity,
        &owner.public_items,
        &owner.trait_contracts,
        id,
        definitions,
    )
}

/// Read the bounded applied parent closure from portable trait contracts.
pub fn interface_ancestors(
    interface: &NominalTy,
    receiver: &Ty,
    closure: &[&BytecodeModule],
    definitions: &DefinitionTable,
) -> Option<Vec<NominalTy>> {
    trait_inheritance::trait_closure(interface, receiver, &CancellationToken::default(), &|id| {
        contract(id, closure, definitions)
    })
    .ok()
}

/// Checked dynamic surfaces; concrete implementation proofs use raw ancestry.
pub fn interface_views(
    interface: &NominalTy,
    receiver: &Ty,
    closure: &[&BytecodeModule],
    definitions: &DefinitionTable,
) -> Option<Vec<NominalTy>> {
    trait_inheritance::interface_views(
        interface,
        receiver,
        &CancellationToken::default(),
        definitions,
        &|id| contract(id, closure, definitions),
    )
    .ok()
}

fn executable_interface(
    scope: &[GenericParam],
    applied: &NominalTy,
    closure: &[&BytecodeModule],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> bool {
    dynamic_interface_valid(
        scope,
        applied,
        &|id| contract(id, closure, definitions),
        definitions,
        cancel,
    )
    .unwrap_or(false)
}

fn declarations<'a>(
    module: &'a BytecodeModule,
    definitions: &DefinitionTable,
) -> Result<Vec<(DefinitionId, &'a TraitDef)>, LinkedValidationError> {
    let root = definitions
        .lookup_root(&module.identity)
        .ok_or(LinkedValidationError::Contract)?;
    let mut declarations = Vec::new();
    for item in &module.public_items {
        if let PublicItem::Trait(record) = item {
            let id = definitions
                .lookup_child(root, DefinitionKind::Trait, &record.name, 0)
                .ok_or(LinkedValidationError::Contract)?;
            declarations.push((id, record));
        }
    }
    declarations.extend(
        module
            .trait_contracts
            .iter()
            .map(|record| (record.declaration, &record.abi)),
    );
    Ok(declarations)
}

pub(super) fn verify_trait_bounds(
    module: usize,
    program: Option<&BytecodeProgram>,
    resources: &VerificationResources<'_>,
    scope: &VerificationScope<'_>,
    cancel: &CancellationToken,
) -> Result<Vec<NativeApplicationWitness>, BytecodeVerificationError> {
    match linked_bounds_match(module, program, resources, scope, cancel) {
        Ok(true) => native::witnesses(module, resources.modules[module], program, scope, cancel)
            .map_err(|error| LinkedValidationError::from(error).into_verification()),
        Ok(false) => Err(LinkedValidationError::Contract.into_verification()),
        Err(error) => Err(error.into_verification()),
    }
}

enum LinkedValidationError {
    InterfaceTable,
    Contract,
    Cancelled,
}

impl LinkedValidationError {
    fn into_verification(self) -> BytecodeVerificationError {
        match self {
            Self::Cancelled => BytecodeVerificationError::Cancelled,
            Self::InterfaceTable => BytecodeVerificationError::InvalidInterfaceTable,
            Self::Contract => BytecodeVerificationError::InvalidHostInterface(
                "trait output or host bound has no unique valid implementation".into(),
            ),
        }
    }
}

impl From<TypeTransformError> for LinkedValidationError {
    fn from(error: TypeTransformError) -> Self {
        match error {
            TypeTransformError::Cancelled => Self::Cancelled,
            _ => Self::Contract,
        }
    }
}

fn linked_bounds_match(
    index: usize,
    program: Option<&BytecodeProgram>,
    resources: &VerificationResources<'_>,
    scope: &VerificationScope<'_>,
    cancel: &CancellationToken,
) -> Result<bool, LinkedValidationError> {
    let module = resources.modules[index];
    let closure = scope.modules.as_slice();
    let catalog = &scope.catalog;
    let declarations = &resources.declarations[index];
    let tables = &resources.tables[index];
    let definitions = catalog.definitions();
    applications::validate(catalog, module, closure, cancel, definitions)?;
    if !views::valid(module, closure, cancel, definitions)
        .map_err(|_| LinkedValidationError::InterfaceTable)?
    {
        return Err(LinkedValidationError::InterfaceTable);
    }
    if !methods::valid(module, catalog, cancel)
        .map_err(|_| LinkedValidationError::InterfaceTable)?
    {
        return Err(LinkedValidationError::InterfaceTable);
    }
    if !shared::valid(module, closure, program, catalog, cancel)? {
        return Ok(false);
    }
    if !instruction_contracts_match(module, closure, program, definitions, cancel) {
        return Ok(false);
    }
    for function in &module.functions {
        cancel
            .check()
            .map_err(|_| LinkedValidationError::Cancelled)?;
        for instruction in &function.instructions {
            cancel
                .check()
                .map_err(|_| LinkedValidationError::Cancelled)?;
            let primitive = match instruction {
                BytecodeInstruction::Binary {
                    op: BinaryOp::Eq | BinaryOp::NotEq,
                    lhs,
                    rhs,
                    ..
                } => Some((
                    Protocol::PartialEq,
                    vec![ScalarOperand::Slot(*lhs), ScalarOperand::Slot(*rhs)],
                )),
                BytecodeInstruction::PredicateBranch {
                    op: BinaryOp::Eq | BinaryOp::NotEq,
                    lhs,
                    rhs,
                    ..
                }
                | BytecodeInstruction::BinaryOperands {
                    op: BinaryOp::Eq | BinaryOp::NotEq,
                    lhs,
                    rhs,
                    ..
                } => Some((Protocol::PartialEq, vec![*lhs, *rhs])),
                BytecodeInstruction::Call {
                    callee:
                        CallTarget::RuntimePrimitive(
                            primitive @ (RuntimePrimitive::ValueEq | RuntimePrimitive::ValueHash),
                        ),
                    args,
                    ..
                } => Some((
                    if *primitive == RuntimePrimitive::ValueEq {
                        Protocol::PartialEq
                    } else {
                        Protocol::Hash
                    },
                    args.iter().copied().map(ScalarOperand::Slot).collect(),
                )),
                _ => None,
            };
            if let Some((protocol, args)) = primitive {
                let Some(facts) = args
                    .iter()
                    .map(|operand| match *operand {
                        ScalarOperand::Slot(slot) => Some((
                            *function.metadata.slots.get(slot.index())?,
                            function
                                .metadata
                                .semantic
                                .slots
                                .get(&slot.index())
                                .map(Cow::Borrowed),
                        )),
                        ScalarOperand::Constant { constant, ty } => {
                            module
                                .constants
                                .get(constant.index())?
                                .numeric_payload(ty)?;
                            Some((
                                builtin_representation(ty),
                                Some(Cow::Owned(Ty::Builtin(ty))),
                            ))
                        }
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    return Ok(false);
                };
                let facts = facts
                    .iter()
                    .map(|(physical, ty)| (*physical, ty.as_deref()))
                    .collect::<Vec<_>>();
                if !catalog.primitive_comparison_valid(protocol, &facts, cancel)? {
                    return Ok(false);
                }
            }
            if let BytecodeInstruction::MakeInterface {
                target: BytecodeConstruction::Bound(required),
                ..
            } = instruction
            {
                let Some(body) = function.metadata.semantic.generic.as_ref() else {
                    return Ok(false);
                };
                if !required.bound_valid(catalog, &body.parameters, &body.bounds, cancel)? {
                    return Ok(false);
                }
            }
            if let BytecodeInstruction::MakeInterface {
                target:
                    BytecodeConstruction::Declared {
                        module: owner,
                        implementation,
                        arguments,
                        ..
                    },
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
                    PublicItem::InterfaceTable(table)
                        if table.declaration == linked.declaration =>
                    {
                        table.instantiate(
                            arguments,
                            body.map_or(&[], |body| body.parameters.as_slice()),
                            definitions,
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
                        cancel,
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
                let Some(declaration) = contract(&call.interface.declaration, closure, definitions)
                else {
                    return Ok(false);
                };
                let body = function.metadata.semantic.generic.as_ref();
                if !call.check(
                    declaration,
                    catalog,
                    body.map_or(&[], |body| body.parameters.as_slice()),
                    body.map_or(&[], |body| body.bounds.as_slice()),
                    cancel,
                )? {
                    return Ok(false);
                }
                for operation in &call.operations {
                    if !callables::witness_valid(operation, closure, definitions) {
                        return Ok(false);
                    }
                }
            }
        }
        if !function
            .metadata
            .semantic
            .protocol_adapter_valid(function.identity.as_ref(), definitions)
        {
            return Ok(false);
        }
        if let Some(required) = &function.metadata.semantic.protocol_adapter
            && catalog
                .implicit_callable_signature(
                    required,
                    function
                        .metadata
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
    if module
        .native_declarations
        .iter()
        .flat_map(|declaration| &declaration.callable_requirements)
        .any(|required| !catalog.callable_requirement_valid(required))
    {
        return Ok(false);
    }
    for import in &module.native_imports {
        let Some(owner) = closure.iter().find(|owner| {
            definitions
                .resolve(import.instance.declaration)
                .is_ok_and(|view| view.module() == &owner.identity)
        }) else {
            return Ok(false);
        };
        let Some(declaration) = owner
            .native_declarations
            .iter()
            .find(|declaration| declaration.declaration == import.instance.declaration)
        else {
            return Ok(false);
        };
        if !import.matches_declaration(declaration, catalog, cancel)?
            || !views::native_result_valid(import, closure, definitions, cancel)
        {
            return Ok(false);
        }
        if import
            .callables
            .iter()
            .any(|call| !callables::witness_valid(call, closure, definitions))
        {
            return Ok(false);
        }
    }
    // Validate unused declaration graphs and constructor references too.
    for &(id, record) in declarations {
        cancel
            .check()
            .map_err(|_| LinkedValidationError::Cancelled)?;
        let applied = NominalTy {
            declaration: id,
            arguments: record
                .generic_params
                .iter()
                .map(GenericParam::as_type)
                .collect(),
            associated_types: Default::default(),
        };
        catalog.ancestry(&applied, &Ty::SelfType(id), cancel)?;
    }
    for table in tables {
        cancel
            .check()
            .map_err(|_| LinkedValidationError::Cancelled)?;
        let Ty::Trait(applied) = &table.trait_type else {
            return Ok(false);
        };
        if !parents_proven(
            applied,
            &table.for_type,
            &table.bounds,
            catalog,
            closure,
            cancel,
        )? {
            return Ok(false);
        }
    }

    for item in &module.public_items {
        cancel
            .check()
            .map_err(|_| LinkedValidationError::Cancelled)?;
        let PublicItem::InterfaceTable(table) = item else {
            continue;
        };
        let Ty::Trait(interface) = &table.trait_type else {
            return Ok(false);
        };
        let Some(record) = contract(&interface.declaration, closure, definitions) else {
            return Ok(false);
        };
        if interface.associated_types.len()
            != record
                .associated_types
                .iter()
                .filter(|member| member.generic_params.is_empty())
                .count()
            || !verify::interface_constants_match(table, record, definitions)
            || !verify::interface_families_match(table, record, cancel, definitions)
            || !verify::interface_methods_match(
                table,
                record,
                &|ty| catalog.normalize(ty, cancel),
                cancel,
            )
        {
            return Ok(false);
        }
        if table.bridge.is_none()
            && matches!(table.for_type, Ty::Host(_))
            && catalog.implementation_count(interface, &table.for_type, &table.bounds, cancel)? != 1
        {
            return Ok(false);
        }
        if !associated_bounds_match(table, interface, record, catalog, closure, cancel)? {
            return Ok(false);
        }
    }
    for linked in &module.interface_tables {
        cancel
            .check()
            .map_err(|_| LinkedValidationError::Cancelled)?;
        if linked.arguments.is_empty() {
            continue;
        }
        let Some(table) = module.public_items.iter().find_map(|item| match item {
            PublicItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                table.instantiate(&linked.arguments, &table.generic_params, definitions)
            }
            _ => None,
        }) else {
            return Ok(false);
        };
        let Ty::Trait(interface) = &table.trait_type else {
            return Ok(false);
        };
        if catalog.implementation_count(interface, &table.for_type, &table.bounds, cancel)? != 1 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn parents_proven(
    interface: &NominalTy,
    receiver: &Ty,
    bounds: &[GenericBound],
    catalog: &ProofCatalog<'_>,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    let definitions = catalog.definitions();
    // Offline host declarations may advertise traits outside this program.
    if matches!(receiver, Ty::Host(_))
        && Protocol::from_definition(&interface.declaration, definitions).is_none()
        && contract(&interface.declaration, closure, definitions).is_none()
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
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> bool {
    for (function, instruction) in module.functions.iter().flat_map(|function| {
        function
            .instructions
            .iter()
            .map(move |instruction| (function, instruction))
    }) {
        if cancel.check().is_err() {
            return false;
        }
        if let BytecodeInstruction::UpcastInterface { source, target, .. } = instruction {
            let Ok(parents) = trait_inheritance::interface_upcast_targets(
                source,
                &Ty::Trait(source.clone()),
                cancel,
                definitions,
                &|id| contract(id, closure, definitions),
            ) else {
                return false;
            };
            if !parents.contains(target) {
                return false;
            }
        }
        if let BytecodeInstruction::MakeInterface {
            target:
                BytecodeConstruction::Declared {
                    module: owner,
                    implementation,
                    arguments,
                    ..
                },
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
                PublicItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                    table.instantiate(
                        arguments,
                        function
                            .metadata
                            .semantic
                            .generic
                            .as_ref()
                            .map_or(&[][..], |body| body.parameters.as_slice()),
                        definitions,
                    )
                }
                _ => None,
            }) else {
                return false;
            };
            let Ty::Trait(applied) = &table.trait_type else {
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
                closure,
                definitions,
                cancel,
            ) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests;
