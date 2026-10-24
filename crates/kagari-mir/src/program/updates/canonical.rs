//! Complete normalized MIR instructions, not a digest or an edge-only summary.
use crate::{
    codec::wire::instruction::InstructionRecord,
    function::{MirFunction, MirModule},
    ids::{InstanceId, ModuleSlotId},
    instruction::{CallTarget, ConstructionTarget, Instruction, RuntimeHelper},
    program::updates::{call, context::Context, mapping_error},
};
use bincode::{serialize, serialized_size};
use kagari_common::identity::{mapping::DefinitionMappingError, scoped::DefinitionReferences};
use kagari_contract::{
    callable::construction::evidence::{
        ConstructionRequirement, ConstructionSelection, ConstructionWitness,
    },
    types::ConcreteFunctionIdentity,
    updates::{
        BodyApplication, BodyClassification, BodyRecord, BodyRequirement, CallRequirement,
        CanonicalInstruction, CanonicalOperand, Dependency, UnknownDependency, UnknownReason,
        limits::{Budget, UpdateError},
    },
};
use kagari_types::{callable::Signature, ty::Ty};
use serde::Serialize;
use std::collections::HashSet;

const FORMAT: u32 = 0x4d495201;

pub(super) fn extract(
    context: &Context<'_>,
    module: &MirModule,
    function: &MirFunction,
    classification: BodyClassification,
    previous: Option<&BodyRecord>,
    budget: &mut Budget<'_>,
) -> Result<BodyRecord, UpdateError> {
    let modules = context.modules;
    let definitions = context.definitions;
    let cancel = context.cancel;
    budget.records(1)?;
    let signature = Signature {
        params: (0..function.params.len())
            .map(|index| {
                function
                    .semantic
                    .params
                    .get(&index)
                    .cloned()
                    .ok_or(UpdateError::InvalidEvidence)
            })
            .collect::<Result<_, _>>()?,
        result: function
            .semantic
            .result
            .clone()
            .ok_or(UpdateError::InvalidEvidence)?,
    };
    let captures = classification_contract(context, module, function, &classification, budget)?;
    let mut body = BodyRecord {
        requirement: BodyRequirement {
            origin: function.instance.clone(),
            generic: function.semantic.generic.clone(),
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
    let mut header = vec![CanonicalOperand::Block(function.entry.index() as u32)];
    header.push(bytes(
        &(
            function
                .params
                .iter()
                .map(|p| (p.ty, p.local))
                .collect::<Vec<_>>(),
            function.return_type,
            function.locals.iter().map(|l| l.ty).collect::<Vec<_>>(),
            function.temps.iter().map(|t| t.ty).collect::<Vec<_>>(),
        ),
        budget,
    )?);
    for map in [
        &function.semantic.params,
        &function.semantic.locals,
        &function.semantic.registers,
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
    for (block_index, block) in function.blocks.iter().enumerate() {
        budget.visit()?;
        body.actual_instructions.push(CanonicalInstruction {
            opcode: 1,
            operands: vec![CanonicalOperand::Block(block_index as u32)],
        });
        for instruction in &block.instructions {
            budget.visit()?;
            let mut normalized = instruction.clone();
            let mut operands = Vec::new();
            match &mut normalized {
                Instruction::Call { callee, .. } => {
                    let requirement = match callee {
                        CallTarget::Function(target) => {
                            let target = module
                                .functions
                                .get(target.index())
                                .ok_or(UpdateError::InvalidEvidence)?;
                            let requirement = function_call(context, target);
                            *callee = CallTarget::Function(InstanceId::new(0));
                            Some(requirement)
                        }
                        CallTarget::SourceFunction(contract) => {
                            Some(CallRequirement::Direct(ConcreteFunctionIdentity {
                                declaration: contract.declaration,
                                arguments: contract.arguments.clone(),
                            }))
                        }
                        CallTarget::Shared(contract) => {
                            Some(CallRequirement::Shared(contract.clone()))
                        }
                        CallTarget::InterfaceMethod(contract) => {
                            Some(CallRequirement::Interface(contract.clone()))
                        }
                        CallTarget::Native(import) => Some(CallRequirement::Native(import.clone())),
                        CallTarget::Value(_) | CallTarget::Closure { .. } => {
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
                    // The checked call operand owns the full semantic contract. The
                    // byte record retains dynamic registers and physical signatures.
                    if matches!(
                        callee,
                        CallTarget::Shared(_)
                            | CallTarget::InterfaceMethod(_)
                            | CallTarget::Native(_)
                            | CallTarget::SourceFunction(_)
                    ) {
                        *callee = CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall);
                    }
                }
                Instruction::MakeClosure {
                    function: target, ..
                }
                | Instruction::MakeFuture {
                    function: target, ..
                } => {
                    let target_function = module
                        .functions
                        .get(target.index())
                        .ok_or(UpdateError::InvalidEvidence)?;
                    let named = context
                        .named
                        .contains(&context.authored(target_function.instance.declaration));
                    let requirement =
                        if named && matches!(instruction, Instruction::MakeClosure { .. }) {
                            CallRequirement::Direct(named_application(
                                context,
                                &target_function.instance,
                            ))
                        } else {
                            CallRequirement::RetainedBody(BodyApplication {
                                body: target_function.instance.clone(),
                                arguments: target_function
                                    .semantic
                                    .generic
                                    .as_ref()
                                    .map_or_else(Vec::new, |body| {
                                        body.parameters.iter().map(|p| p.as_type()).collect()
                                    }),
                            })
                        };
                    add_call(&mut body, &mut operands, requirement, budget)?;
                    *target = InstanceId::new(0);
                }
                Instruction::LoadModule { slot, .. } | Instruction::StoreModule { slot, .. } => {
                    let declaration = module
                        .module_slots
                        .get(slot.index())
                        .ok_or(UpdateError::InvalidEvidence)?
                        .contract
                        .declaration;
                    operands.push(CanonicalOperand::Declaration(declaration));
                    body.dependencies.push(Dependency::Source(declaration));
                    *slot = ModuleSlotId::new(0);
                }
                Instruction::ReadPath { path, .. }
                | Instruction::SetPath { path, .. }
                | Instruction::ModifyPath { path, .. }
                | Instruction::MakePathView { path, .. } => {
                    if path.declaration.is_none() {
                        return Err(UpdateError::InvalidEvidence);
                    }
                    path.debug_name.clear();
                    add_call(
                        &mut body,
                        &mut operands,
                        CallRequirement::Unknown(UnknownReason::NativeReentry),
                        budget,
                    )?;
                }
                _ => {}
            }
            if let Instruction::MakeInterface { dst, value, target } = instruction {
                let witness = match target {
                    ConstructionTarget::Bound(requirement) => ConstructionWitness {
                        requirement: requirement.clone(),
                        selection: ConstructionSelection::Forward,
                    },
                    ConstructionTarget::Declared {
                        implementation,
                        arguments,
                    } => {
                        let receiver = function
                            .semantic
                            .registers
                            .get(&value.temp.index())
                            .ok_or(UpdateError::InvalidEvidence)?;
                        let Some(Ty::Trait(interface)) =
                            function.semantic.registers.get(&dst.temp.index())
                        else {
                            return Err(UpdateError::InvalidEvidence);
                        };
                        context.construction(
                            &ConstructionRequirement {
                                receiver: receiver.clone(),
                                interface: interface.clone(),
                            },
                            &ConcreteFunctionIdentity {
                                declaration: *implementation,
                                arguments: arguments.clone(),
                            },
                            function.semantic.generic.as_ref(),
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
                Instruction::MakeStruct { structure, .. } => Some(Ty::Struct(structure.clone())),
                Instruction::MakeEnum { enumeration, .. }
                | Instruction::TestEnumVariant { enumeration, .. }
                | Instruction::ReadEnumPayload { enumeration, .. } => {
                    Some(Ty::Enum(enumeration.clone()))
                }
                Instruction::ReadAggregateField { field, .. }
                | Instruction::WriteAggregateField { field, .. } => {
                    Some(Ty::Struct(field.owner.clone()))
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
            let mut references = HashSet::new();
            normalized.visit_definition_ids(
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
            operands.push(bytes(
                &InstructionRecord::encode(&normalized, &encoding, cancel)?,
                budget,
            )?);
            body.actual_instructions.push(CanonicalInstruction {
                opcode: 2,
                operands,
            });
        }
        body.actual_instructions.push(CanonicalInstruction {
            opcode: 3,
            operands: vec![bytes(
                block
                    .terminator
                    .as_ref()
                    .ok_or(UpdateError::InvalidEvidence)?,
                budget,
            )?],
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
            for import in modules
                .iter()
                .flat_map(|module| module.native_applications())
            {
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

fn named_application(
    context: &Context<'_>,
    instance: &ConcreteFunctionIdentity,
) -> ConcreteFunctionIdentity {
    let mut application = instance.clone();
    application.declaration = context.authored(instance.declaration);
    application
}

fn function_call(context: &Context<'_>, function: &MirFunction) -> CallRequirement {
    let application = named_application(context, &function.instance);
    if context.named.contains(&application.declaration) {
        CallRequirement::Direct(application)
    } else {
        CallRequirement::RetainedBody(BodyApplication {
            body: function.instance.clone(),
            arguments: function
                .semantic
                .generic
                .as_ref()
                .map_or_else(Vec::new, |body| {
                    body.parameters
                        .iter()
                        .map(|parameter| parameter.as_type())
                        .collect()
                }),
        })
    }
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
    module: &MirModule,
    function: &MirFunction,
    classification: &BodyClassification,
    budget: &mut Budget<'_>,
) -> Result<Vec<Ty>, UpdateError> {
    let authored = named_application(context, &function.instance).declaration;
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
    | BodyClassification::Resume(parent) = classification
        && (parent == &function.instance || !context.bodies.contains(parent))
    {
        return Err(UpdateError::InvalidEvidence);
    }
    if let BodyClassification::StaticInitializer(id) = classification
        && !module.module_slots.iter().any(|slot| {
            slot.contract.supplier == module.identity
                && slot.contract.declaration == *id
                && slot.initializer == function.id
                && slot.contract.accepts_initializer_identity(
                    function.instance.declaration,
                    context.definitions,
                )
        })
    {
        return Err(UpdateError::InvalidEvidence);
    }
    if let BodyClassification::Adapter(identity) = classification
        && (identity != &function.instance
            || (function.semantic.protocol_adapter.is_none()
                && !context.bridges.contains(identity)))
    {
        return Err(UpdateError::InvalidEvidence);
    }
    let mut captures = None;
    for creation in context
        .creations
        .get(&function.instance)
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
            if function.semantic.params.get(&index) != Some(ty) {
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
