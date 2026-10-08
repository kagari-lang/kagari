use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction},
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionPath};
use kagari_contract::{
    callable::witness::OperationWitness,
    types::applications::{validate_declarations, validate_layouts, validate_slots},
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
    lookup: impl Fn(&DefinitionPath) -> Option<&'a TraitDef>,
    nominal: impl Fn(&DefinitionPath) -> Option<(TypeDefKind, usize)>,
) -> Result<(), TypeTransformError> {
    let validator = ApplicationValidator::new(cancel, lookup, &nominal);
    validate_declarations(
        &validator,
        &module.abi.public_items,
        &module.abi.trait_contracts,
        cancel,
    )?;
    for declaration in &module.abi.native_declarations {
        validator.function(&declaration.function)?;
        for required in &declaration.callable_requirements {
            validator.validate_type(&required.receiver)?;
            validator.trait_application(&required.interface)?;
            validator.types(&required.arguments)?;
        }
    }
    for import in module.native_applications() {
        validator.types(&import.instance.arguments)?;
        validator.types(&import.signature.params)?;
        validator.validate_type(&import.signature.result)?;
        validator.bounds(&import.requirements)?;
        for operation in &import.callables {
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
            validator.types(&call.signature.params)?;
            validator.validate_type(&call.signature.result)?;
        }
    }
    validate_layouts(&validator, &module.structures, &module.enumerations, cancel)?;
    for instance in &module.interface_instances {
        validator.types(&instance.arguments)?;
    }
    for function in &module.functions {
        validator.types(&function.instance.arguments)?;
        validate_slots(&validator, &function.semantic, cancel)?;
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            match instruction {
                Instruction::Await { future, .. } | Instruction::MakeFuture { future, .. } => {
                    validator.validate_type(future)?;
                    let Ty::NativeObject(future) = future else {
                        return Err(TypeTransformError::InvalidContract);
                    };
                    if nominal(&future.declaration)
                        != Some((TypeDefKind::NativeStorage(NativeStorageLayout::Future), 1))
                    {
                        return Err(TypeTransformError::InvalidContract);
                    }
                }
                Instruction::Call {
                    callee: CallTarget::Shared(contract),
                    ..
                } => {
                    validator.types(&contract.instance.arguments)?;
                    validator.types(&contract.arguments)?;
                    validator.types(&contract.signature.params)?;
                    validator.validate_type(&contract.signature.result)?;
                    for operation in &contract.operations {
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
                Instruction::MakeInterface { arguments, .. } => validator.types(arguments)?,
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
