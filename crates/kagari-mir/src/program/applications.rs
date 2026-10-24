use crate::{
    function::MirModule,
    instruction::{CallTarget, ConstructionTarget as MirConstruction, Instruction},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};
use kagari_contract::{
    callable::witness::OperationWitness,
    types::applications::{
        validate_declarations, validate_function, validate_layouts, validate_slots, validate_types,
    },
};
use kagari_types::{
    declaration::{
        TraitDef, TypeDefKind, applications::ApplicationValidator, native::NativeStorageLayout,
    },
    ty::{Ty, substitution::TypeTransformError},
};

pub(super) fn validate<'a>(
    module: &MirModule,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
    lookup: impl Fn(&DefinitionId) -> Option<&'a TraitDef>,
    nominal: impl Fn(&DefinitionId) -> Option<(TypeDefKind, usize)>,
) -> Result<(), TypeTransformError> {
    let validator = ApplicationValidator::new(cancel, lookup, &nominal);
    validate_declarations(
        &validator,
        &module.abi.public_items,
        &module.abi.trait_contracts,
        &module.abi.private_types,
        &module.abi.named_declarations,
        cancel,
        definitions,
    )?;
    for declaration in &module.abi.native_declarations {
        validate_function(&validator, &declaration.function, cancel, definitions)?;
        for required in &declaration.callable_requirements {
            validator.validate_type(&required.receiver)?;
            validator.trait_application(&required.interface)?;
            validator.types(&required.arguments)?;
        }
    }
    for import in module.native_applications() {
        validator.types(&import.instance.arguments)?;
        validate_types(&validator, &import.signature.params, cancel, definitions)?;
        validate_types(&validator, [&import.signature.result], cancel, definitions)?;
        validator.bounds(&import.requirements)?;
        for operation in import.callables.iter().flat_map(OperationWitness::walk) {
            let required = operation.requirement();
            validator.validate_type(&required.receiver)?;
            validator.trait_application(&required.interface)?;
            validator.types(&required.arguments)?;
            let OperationWitness::Selected(call) = operation else {
                continue;
            };
            validator.types(&call.instance.arguments)?;
            validator.validate_type(&call.requirement.receiver)?;
            validator.trait_application(&call.requirement.interface)?;
            validator.types(&call.requirement.arguments)?;
            validate_types(&validator, &call.signature.params, cancel, definitions)?;
            validate_types(&validator, [&call.signature.result], cancel, definitions)?;
        }
    }
    validate_layouts(
        &validator,
        &module.structures,
        &module.enumerations,
        cancel,
        definitions,
    )?;
    for instance in &module.interface_instances {
        validator.types(&instance.arguments)?;
    }
    for function in &module.functions {
        validator.types(&function.instance.arguments)?;
        validate_slots(&validator, &function.semantic, cancel, definitions)?;
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            match instruction {
                Instruction::Await { future, .. } | Instruction::MakeFuture { future, .. } => {
                    validator.validate_type(future)?;
                    let Ty::NativeObject(future) = future else {
                        return Err(TypeTransformError::InvalidContract);
                    };
                    let valid = match nominal(&future.declaration) {
                        Some((TypeDefKind::NativeStorage(NativeStorageLayout::Future), 1)) => true,
                        Some((TypeDefKind::NativeStorage(NativeStorageLayout::Task), 1)) => {
                            matches!(instruction, Instruction::Await { .. })
                        }
                        _ => false,
                    };
                    if !valid {
                        return Err(TypeTransformError::InvalidContract);
                    }
                }
                Instruction::Call {
                    callee: CallTarget::Shared(contract),
                    ..
                } => {
                    validator.types(&contract.instance.arguments)?;
                    validator.types(&contract.arguments)?;
                    validate_types(&validator, &contract.signature.params, cancel, definitions)?;
                    validate_types(
                        &validator,
                        [&contract.signature.result],
                        cancel,
                        definitions,
                    )?;
                    for operation in contract.operations.iter().flat_map(OperationWitness::walk) {
                        let required = operation.requirement();
                        validator.validate_type(&required.receiver)?;
                        validator.trait_application(&required.interface)?;
                        validator.types(&required.arguments)?;
                    }
                }
                Instruction::MakeArray { element, .. }
                | Instruction::RepeatArray { element, .. } => validator.validate_type(element)?,
                Instruction::Iter { ty, .. } | Instruction::MakeRange { ty, .. } => {
                    validator.validate_type(ty)?
                }
                Instruction::RangeBound { range, bound, .. } => validator.types([range, bound])?,
                Instruction::UpcastInterface { source, target, .. } => {
                    validator.trait_application(source)?;
                    validator.trait_application(target)?;
                }
                Instruction::MakeInterface { target, .. } => match target {
                    MirConstruction::Declared { arguments, .. } => validator.types(arguments)?,
                    MirConstruction::Bound(required) => {
                        validator.validate_type(&required.receiver)?;
                        validator.trait_application(&required.interface)?;
                    }
                },
                Instruction::MakeStruct { structure, .. } => {
                    validator.nominal_arguments(structure)?
                }
                Instruction::MakeEnum { enumeration, .. }
                | Instruction::TestEnumVariant { enumeration, .. }
                | Instruction::ReadEnumPayload { enumeration, .. } => {
                    validator.nominal_arguments(enumeration)?
                }
                Instruction::ReadAggregateField { field, .. }
                | Instruction::WriteAggregateField { field, .. } => {
                    validator.nominal_arguments(&field.owner)?
                }
                Instruction::Call {
                    callee: CallTarget::SourceFunction(contract),
                    ..
                } => validator.types(&contract.arguments)?,
                Instruction::Call {
                    callee: CallTarget::InterfaceMethod(contract),
                    ..
                } => validator.trait_application(&contract.interface)?,
                _ => {}
            }
        }
    }
    Ok(())
}
