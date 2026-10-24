//! Canonical emitted bytecode, with all non-local slots resolved to semantic facts.
use crate::{
    artifact::wire::instruction::canonical_record,
    instruction::{
        BytecodeInstruction, CallTarget, ConstantId, ConstructionTarget, EnumId, InterfaceTableRef,
        ModuleSlot, PathId, RuntimeHelper, ScalarOperand, StructId,
    },
    module::{BytecodeFunction, BytecodeModule, CallableTarget},
    program::{
        ModuleRef,
        updates::{call, context::Context, mapping_error},
    },
};
use bincode::{serialize, serialized_size};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
        table::{DefinitionTable, wire::DefinitionEncoding},
    },
};
use kagari_contract::{
    callable::construction::evidence::{
        ConstructionRequirement, ConstructionSelection, ConstructionWitness,
    },
    ids::FunctionRef,
    types::ConcreteFunctionIdentity,
    updates::{
        BodyApplication, BodyClassification, BodyRecord, BodyRequirement, CallRequirement,
        CanonicalInstruction, CanonicalOperand, Dependency, UnknownDependency, UnknownReason,
        limits::{Budget, UpdateError},
    },
};
use kagari_types::{
    callable::Signature,
    declaration::wire::path::PathDeclarationRecord,
    ty::{NominalTy, Ty},
};
use serde::Serialize;
use std::collections::HashSet;

const FORMAT: u32 = 0x4b424301;

pub(super) fn extract(
    context: &Context<'_>,
    module: &BytecodeModule,
    function: &BytecodeFunction,
    classification: BodyClassification,
    previous: Option<&BodyRecord>,
    budget: &mut Budget<'_>,
) -> Result<BodyRecord, UpdateError> {
    let modules = context.modules;
    let definitions = context.definitions;
    let cancel = context.cancel;
    budget.records(1)?;
    let identity = function
        .identity
        .as_ref()
        .ok_or(UpdateError::InvalidEvidence)?;
    let signature = Signature {
        params: (0..function.parameter_count as usize)
            .map(|index| {
                function
                    .metadata
                    .semantic
                    .params
                    .get(&index)
                    .cloned()
                    .ok_or(UpdateError::InvalidEvidence)
            })
            .collect::<Result<_, _>>()?,
        result: function
            .metadata
            .semantic
            .result
            .clone()
            .ok_or(UpdateError::InvalidEvidence)?,
    };
    let captures = classification_contract(context, module, function, &classification, budget)?;
    let mut body = BodyRecord {
        requirement: BodyRequirement {
            origin: identity.clone(),
            generic: function.metadata.semantic.generic.clone(),
            calls: previous.map_or_else(Vec::new, |body| body.requirement.calls.clone()),
        },
        classification,
        signature,
        captures,
        format: FORMAT,
        instructions: vec![],
        actual_format: FORMAT,
        actual_instructions: vec![],
        dependencies: vec![],
    };
    let mut header = vec![bytes(
        &(
            function.parameter_count,
            function.slot_count,
            &function.metadata.params,
            function.metadata.return_type,
            &function.metadata.slots,
            &function.metadata.parameter_slots,
        ),
        budget,
    )?];
    for map in [
        &function.metadata.semantic.params,
        &function.metadata.semantic.slots,
    ] {
        header.push(CanonicalOperand::Unsigned(map.len() as u64));
        for (index, ty) in map {
            budget.visit()?;
            header.extend([
                CanonicalOperand::Local(*index as u32),
                CanonicalOperand::Type(ty.clone()),
            ]);
        }
    }
    body.actual_instructions.push(CanonicalInstruction {
        opcode: 0,
        operands: header,
    });
    for instruction in &function.instructions {
        budget.visit()?;
        let mut normalized = instruction.clone();
        let mut operands = Vec::new();
        match &mut normalized {
            BytecodeInstruction::Call { callee, .. } => {
                let requirement = match callee {
                    CallTarget::Function(target) => Some(function_call(context, module, *target)?),
                    CallTarget::ModuleFunction {
                        module: owner,
                        function: target,
                    } => Some(function_call(
                        context,
                        modules
                            .get(owner.index())
                            .ok_or(UpdateError::InvalidEvidence)?,
                        *target,
                    )?),
                    CallTarget::Shared { contract, .. } => {
                        Some(CallRequirement::Shared(contract.clone()))
                    }
                    CallTarget::InterfaceMethod { contract, .. } => {
                        Some(CallRequirement::Interface(contract.clone()))
                    }
                    CallTarget::Native(import) => Some(CallRequirement::Native(Box::new(
                        module
                            .native_imports
                            .get(import.index())
                            .ok_or(UpdateError::InvalidEvidence)?
                            .clone(),
                    ))),
                    CallTarget::Slot(_) | CallTarget::ClosureRegister { .. } => {
                        Some(CallRequirement::Unknown(UnknownReason::FunctionValue))
                    }
                    CallTarget::RuntimeHelper(_) => {
                        Some(CallRequirement::Unknown(UnknownReason::NativeReentry))
                    }
                    CallTarget::RuntimePrimitive(_) => {
                        Some(CallRequirement::Unknown(UnknownReason::NativeReentry))
                    }
                };
                if let Some(requirement) = requirement {
                    add_call(&mut body, &mut operands, requirement, budget)?;
                }
                match callee {
                    CallTarget::Shared {
                        module: owner,
                        target,
                        ..
                    } => {
                        let owner = modules
                            .get(owner.index())
                            .ok_or(UpdateError::InvalidEvidence)?;
                        let application = match target {
                            CallableTarget::Script(target) => {
                                target_identity(context, owner, *target)?
                            }
                            CallableTarget::Native(target) => owner
                                .native_imports
                                .get(target.index())
                                .ok_or(UpdateError::InvalidEvidence)?
                                .instance
                                .clone(),
                        };
                        operands.push(CanonicalOperand::Application(application));
                        *callee = CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall);
                    }
                    CallTarget::Function(_)
                    | CallTarget::ModuleFunction { .. }
                    | CallTarget::InterfaceMethod { .. }
                    | CallTarget::Native(_) => {
                        *callee = CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall)
                    }
                    _ => {}
                }
            }
            BytecodeInstruction::MakeClosure {
                function: target, ..
            }
            | BytecodeInstruction::MakeFuture {
                function: target, ..
            } => {
                let target_function = module
                    .functions
                    .get(target.index())
                    .ok_or(UpdateError::InvalidEvidence)?;
                let application = target_identity(context, module, *target)?;
                let named = context.named.contains(&application.declaration);
                let requirement =
                    if named && matches!(instruction, BytecodeInstruction::MakeClosure { .. }) {
                        CallRequirement::Direct(application)
                    } else {
                        CallRequirement::RetainedBody(BodyApplication {
                            body: target_function
                                .identity
                                .clone()
                                .ok_or(UpdateError::InvalidEvidence)?,
                            arguments: target_function
                                .metadata
                                .semantic
                                .generic
                                .as_ref()
                                .map_or_else(Vec::new, |body| {
                                    body.parameters.iter().map(|p| p.as_type()).collect()
                                }),
                        })
                    };
                add_call(&mut body, &mut operands, requirement, budget)?;
                *target = FunctionRef::new(0);
            }
            BytecodeInstruction::LoadModule { slot, .. }
            | BytecodeInstruction::StoreModule { slot, .. } => {
                let declaration = module
                    .module_slots
                    .get(slot.index())
                    .ok_or(UpdateError::InvalidEvidence)?
                    .contract
                    .declaration;
                operands.push(CanonicalOperand::Declaration(declaration));
                body.dependencies.push(Dependency::Source(declaration));
                *slot = ModuleSlot::new(0);
            }
            BytecodeInstruction::LoadConst { constant, .. } => {
                operands.push(bytes(
                    module
                        .constants
                        .get(constant.index())
                        .ok_or(UpdateError::InvalidEvidence)?,
                    budget,
                )?);
                *constant = ConstantId::new(0);
            }
            BytecodeInstruction::BinaryOperands { lhs, rhs, .. }
            | BytecodeInstruction::PredicateBranch { lhs, rhs, .. } => {
                for operand in [lhs, rhs] {
                    if let ScalarOperand::Constant { constant, .. } = operand {
                        operands.push(bytes(
                            module
                                .constants
                                .get(constant.index())
                                .ok_or(UpdateError::InvalidEvidence)?,
                            budget,
                        )?);
                        *constant = ConstantId::new(0);
                    }
                }
            }
            BytecodeInstruction::MakeStruct { structure, .. } => {
                operands.push(CanonicalOperand::Declaration(
                    module
                        .structures
                        .get(structure.index())
                        .ok_or(UpdateError::InvalidEvidence)?
                        .declaration,
                ));
                *structure = StructId::new(0);
            }
            BytecodeInstruction::MakeEnum { enumeration, .. }
            | BytecodeInstruction::TestEnumVariant { enumeration, .. }
            | BytecodeInstruction::ReadEnumPayload { enumeration, .. } => {
                operands.push(CanonicalOperand::Declaration(
                    module
                        .enumerations
                        .get(enumeration.index())
                        .ok_or(UpdateError::InvalidEvidence)?
                        .declaration,
                ));
                *enumeration = EnumId::new(0);
            }
            BytecodeInstruction::ReadAggregateField { field, .. }
            | BytecodeInstruction::WriteAggregateField { field, .. } => {
                operands.push(CanonicalOperand::Declaration(
                    module
                        .structures
                        .get(field.structure.index())
                        .ok_or(UpdateError::InvalidEvidence)?
                        .declaration,
                ));
                field.structure = StructId::new(0);
            }
            BytecodeInstruction::MakeInterface {
                target:
                    ConstructionTarget::Declared {
                        module: owner,
                        implementation,
                        ..
                    },
                ..
            } => {
                let table = modules
                    .get(owner.index())
                    .and_then(|module| module.interface_tables.get(implementation.index()))
                    .ok_or(UpdateError::InvalidEvidence)?;
                operands.push(CanonicalOperand::Declaration(table.declaration));
                *owner = ModuleRef::new(0);
                *implementation = InterfaceTableRef::new(0);
            }
            BytecodeInstruction::ReadPath { path, .. }
            | BytecodeInstruction::SetPath { path, .. }
            | BytecodeInstruction::ModifyPath { path, .. }
            | BytecodeInstruction::MakePathView { path, .. } => {
                let record = module
                    .paths
                    .get(path.index())
                    .ok_or(UpdateError::InvalidEvidence)?;
                let mut found = None;
                for declaration in &module.path_declarations {
                    budget.visit()?;
                    if declaration
                        .contract(&module.external_types, definitions)?
                        .fingerprint(definitions)?
                        == record.contract_fingerprint
                    {
                        found = Some(declaration);
                        break;
                    }
                }
                let declaration = found.ok_or(UpdateError::InvalidEvidence)?;
                let encoding = references(declaration, &mut operands, definitions, cancel, budget)?;
                operands.push(bytes(
                    &PathDeclarationRecord::encode(declaration, &encoding, cancel)?,
                    budget,
                )?);
                operands.push(bytes(
                    &(record.root_ty, record.result_ty, record.read_only),
                    budget,
                )?);
                add_call(
                    &mut body,
                    &mut operands,
                    CallRequirement::Unknown(UnknownReason::NativeReentry),
                    budget,
                )?;
                *path = PathId::new(0);
            }
            _ => {}
        }
        if let BytecodeInstruction::MakeInterface { dst, value, target } = instruction {
            let witness = match target {
                ConstructionTarget::Bound(requirement) => ConstructionWitness {
                    requirement: requirement.clone(),
                    selection: ConstructionSelection::Forward,
                },
                ConstructionTarget::Declared {
                    module: owner,
                    implementation,
                    arguments,
                } => {
                    let owner = modules
                        .get(owner.index())
                        .ok_or(UpdateError::InvalidEvidence)?;
                    let declaration = owner
                        .interface_tables
                        .get(implementation.index())
                        .ok_or(UpdateError::InvalidEvidence)?
                        .declaration;
                    let semantic = &function.metadata.semantic;
                    let receiver = semantic
                        .slots
                        .get(&value.index())
                        .ok_or(UpdateError::InvalidEvidence)?;
                    let Some(Ty::Trait(interface)) = semantic.slots.get(&dst.index()) else {
                        return Err(UpdateError::InvalidEvidence);
                    };
                    context.construction(
                        &ConstructionRequirement {
                            receiver: receiver.clone(),
                            interface: interface.clone(),
                        },
                        &ConcreteFunctionIdentity {
                            declaration,
                            arguments: arguments.clone(),
                        },
                        semantic.generic.as_ref(),
                        budget,
                    )?
                }
            };
            add_call(
                &mut body,
                &mut operands,
                CallRequirement::Construction(Box::new(witness)),
                budget,
            )?;
        }
        let layout = match instruction {
            BytecodeInstruction::MakeStruct {
                structure,
                arguments,
                ..
            } => Some(Ty::Struct(NominalTy {
                declaration: module
                    .structures
                    .get(structure.index())
                    .ok_or(UpdateError::InvalidEvidence)?
                    .declaration,
                arguments: arguments.clone(),
                associated_types: Default::default(),
            })),
            BytecodeInstruction::MakeEnum {
                enumeration,
                arguments,
                ..
            }
            | BytecodeInstruction::TestEnumVariant {
                enumeration,
                arguments,
                ..
            }
            | BytecodeInstruction::ReadEnumPayload {
                enumeration,
                arguments,
                ..
            } => Some(Ty::Enum(NominalTy {
                declaration: module
                    .enumerations
                    .get(enumeration.index())
                    .ok_or(UpdateError::InvalidEvidence)?
                    .declaration,
                arguments: arguments.clone(),
                associated_types: Default::default(),
            })),
            BytecodeInstruction::ReadAggregateField { field, .. }
            | BytecodeInstruction::WriteAggregateField { field, .. } => {
                Some(Ty::Struct(NominalTy {
                    declaration: module
                        .structures
                        .get(field.structure.index())
                        .ok_or(UpdateError::InvalidEvidence)?
                        .declaration,
                    arguments: field.arguments.clone(),
                    associated_types: Default::default(),
                }))
            }
            _ => None,
        };
        if let Some(layout) = layout {
            add_call(
                &mut body,
                &mut operands,
                CallRequirement::Layout(layout),
                budget,
            )?;
        }
        let encoding = references(&normalized, &mut operands, definitions, cancel, budget)?;
        operands.push(bytes(
            &canonical_record(&normalized, &encoding, cancel)?,
            budget,
        )?);
        body.actual_instructions.push(CanonicalInstruction {
            opcode: 1,
            operands,
        });
    }
    for dependency in &mut body.dependencies {
        budget.visit()?;
        if let Dependency::Source(declaration) = dependency {
            *declaration = context.source(*declaration);
        }
        let Dependency::Body(target) = dependency else {
            continue;
        };
        if context.bodies.contains(target) {
            continue;
        }
        let edge = body.requirement.calls.iter().position(
            |call| matches!(call, CallRequirement::Shared(call) if &call.instance == target),
        );
        if let Some(edge) = edge {
            let mut native = false;
            for import in modules.iter().flat_map(|module| &module.native_imports) {
                budget.visit()?;
                native |= &import.instance == target;
            }
            if native {
                *dependency = Dependency::Unknown(UnknownDependency { edge: edge as u32 });
            }
        }
    }
    body.instructions = body.actual_instructions.clone();
    Ok(body)
}

fn references(
    value: &impl DefinitionReferences,
    operands: &mut Vec<CanonicalOperand>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
    budget: &mut Budget<'_>,
) -> Result<DefinitionEncoding, UpdateError> {
    let mut references = HashSet::new();
    value.visit_definition_ids(
        &mut |id| {
            budget.visit().map_err(mapping_error)?;
            references.insert(*id);
            Ok(())
        },
        cancel,
    )?;
    let encoding = definitions
        .encode(references.iter().copied())
        .map_err(DefinitionMappingError::from)?;
    let mut references = references
        .into_iter()
        .map(|id| {
            Ok((
                encoding
                    .reference(id)
                    .map_err(DefinitionMappingError::from)?
                    .index(),
                id,
            ))
        })
        .collect::<Result<Vec<_>, UpdateError>>()?;
    references.sort_by_key(|(index, _)| *index);
    operands.push(CanonicalOperand::Unsigned(references.len() as u64));
    for (index, id) in references {
        operands.extend([
            CanonicalOperand::Unsigned(index as u64),
            CanonicalOperand::Declaration(id),
        ]);
    }
    Ok(encoding)
}

fn target_identity(
    context: &Context<'_>,
    module: &BytecodeModule,
    target: FunctionRef,
) -> Result<ConcreteFunctionIdentity, UpdateError> {
    let mut application = module
        .functions
        .get(target.index())
        .and_then(|function| function.identity.clone())
        .ok_or(UpdateError::InvalidEvidence)?;
    application.declaration = context.authored(application.declaration);
    Ok(application)
}

fn function_call(
    context: &Context<'_>,
    module: &BytecodeModule,
    target: FunctionRef,
) -> Result<CallRequirement, UpdateError> {
    let application = target_identity(context, module, target)?;
    if context.named.contains(&application.declaration) {
        return Ok(CallRequirement::Direct(application));
    }
    let function = module
        .functions
        .get(target.index())
        .ok_or(UpdateError::InvalidEvidence)?;
    Ok(CallRequirement::RetainedBody(BodyApplication {
        body: function
            .identity
            .clone()
            .ok_or(UpdateError::InvalidEvidence)?,
        arguments: function
            .metadata
            .semantic
            .generic
            .as_ref()
            .map_or_else(Vec::new, |body| {
                body.parameters
                    .iter()
                    .map(|parameter| parameter.as_type())
                    .collect()
            }),
    }))
}

fn add_call(
    body: &mut BodyRecord,
    operands: &mut Vec<CanonicalOperand>,
    requirement: CallRequirement,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    let operand = call(&mut body.requirement.calls, requirement.clone(), budget)?;
    let CanonicalOperand::Call(edge) = operand else {
        return Err(UpdateError::InvalidEvidence);
    };
    let dependency = match &requirement {
        CallRequirement::Direct(target) => Some(Dependency::Source(target.declaration)),
        CallRequirement::RetainedBody(target) => Some(Dependency::Body(target.body.clone())),
        CallRequirement::Shared(target) => Some(Dependency::Body(target.instance.clone())),
        CallRequirement::Interface(_)
        | CallRequirement::Native(_)
        | CallRequirement::Operation(_)
        | CallRequirement::Construction(_)
        | CallRequirement::Unknown(_) => Some(Dependency::Unknown(UnknownDependency { edge })),
        _ => None,
    };
    if let Some(dependency) = dependency
        && !body.dependencies.contains(&dependency)
    {
        body.dependencies.push(dependency);
    }
    operands.push(CanonicalOperand::Call(edge));
    Ok(())
}

fn classification_contract(
    context: &Context<'_>,
    module: &BytecodeModule,
    function: &BytecodeFunction,
    classification: &BodyClassification,
    budget: &mut Budget<'_>,
) -> Result<Vec<Ty>, UpdateError> {
    let authored = target_identity(context, module, function.id)?.declaration;
    let named = context.named.contains(&authored);
    if let BodyClassification::Named(id) = classification {
        if !named || *id != authored {
            return Err(UpdateError::InvalidEvidence);
        }
    } else if named {
        return Err(UpdateError::InvalidEvidence);
    }
    if module
        .module_slots
        .iter()
        .any(|slot| slot.contract.supplier == module.identity && slot.initializer == function.id)
        && !matches!(classification, BodyClassification::StaticInitializer(_))
    {
        return Err(UpdateError::InvalidEvidence);
    }
    if let BodyClassification::Closure(parent)
    | BodyClassification::Async(parent)
    | BodyClassification::Resume(parent)
    | BodyClassification::Adapter(parent) = classification
        && (Some(parent) == function.identity.as_ref() || !context.bodies.contains(parent))
    {
        return Err(UpdateError::InvalidEvidence);
    }
    if let BodyClassification::StaticInitializer(id) = classification
        && !module.module_slots.iter().any(|slot| {
            slot.contract.supplier == module.identity
                && slot.contract.declaration == *id
                && slot.initializer == function.id
                && function.identity.as_ref().is_some_and(|identity| {
                    slot.contract
                        .accepts_initializer_identity(identity.declaration, context.definitions)
                })
        })
    {
        return Err(UpdateError::InvalidEvidence);
    }
    if let BodyClassification::IntrinsicAdapter(identity) = classification
        && (Some(identity) != function.identity.as_ref()
            || (function.metadata.semantic.protocol_adapter.is_none()
                && !context.bridges.contains(identity)))
    {
        return Err(UpdateError::InvalidEvidence);
    }
    // A source-local adapter cannot erase its authored owner by claiming to be
    // intrinsic. Only module-owned protocol recipes or verified bridge methods qualify.
    if let BodyClassification::IntrinsicAdapter(identity) = classification
        && !context.bridges.contains(identity)
        && context
            .definitions
            .parent(identity.declaration)
            .ok()
            .flatten()
            != context.definitions.lookup_root(&module.identity)
    {
        return Err(UpdateError::InvalidEvidence);
    }
    let mut captures = None;
    for creation in context
        .creations
        .get(
            function
                .identity
                .as_ref()
                .ok_or(UpdateError::InvalidEvidence)?,
        )
        .into_iter()
        .flatten()
    {
        budget.visit()?;
        if (creation.future
            && !matches!(
                classification,
                BodyClassification::Resume(_) | BodyClassification::Async(_)
            ))
            || (!creation.future && matches!(classification, BodyClassification::Resume(_)))
        {
            return Err(UpdateError::InvalidEvidence);
        }
        match classification {
            BodyClassification::Closure(parent)
            | BodyClassification::Async(parent)
            | BodyClassification::Resume(parent)
            | BodyClassification::Adapter(parent)
                if parent != &creation.parent =>
            {
                return Err(UpdateError::InvalidEvidence);
            }
            BodyClassification::Named(_) if !creation.captures.is_empty() => {
                return Err(UpdateError::InvalidEvidence);
            }
            _ => {}
        }
        if captures
            .as_ref()
            .is_some_and(|previous| previous != &creation.captures)
        {
            return Err(UpdateError::InvalidEvidence);
        }
        captures = Some(creation.captures.clone());
    }
    if let Some(types) = &captures {
        for (index, ty) in types.iter().enumerate() {
            if function.metadata.semantic.params.get(&index) != Some(ty) {
                return Err(UpdateError::InvalidEvidence);
            }
        }
    }
    Ok(captures.unwrap_or_default())
}

fn bytes(value: &impl Serialize, budget: &mut Budget<'_>) -> Result<CanonicalOperand, UpdateError> {
    let size = serialized_size(value).map_err(|_| UpdateError::InvalidEvidence)?;
    budget.bytes(usize::try_from(size).map_err(|_| UpdateError::LimitExceeded)?)?;
    Ok(CanonicalOperand::Bytes(
        serialize(value).map_err(|_| UpdateError::InvalidEvidence)?,
    ))
}
