use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction},
};

use kagari_abi::types::{
    TraitAbi, applications::ApplicationValidator, substitution::TypeTransformError,
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};

pub(super) fn validate<'a>(
    module: &MirModule,
    cancel: &CancellationToken,
    lookup: impl Fn(&DefinitionId) -> Option<&'a TraitAbi>,
) -> Result<(), TypeTransformError> {
    let validator = ApplicationValidator::new(cancel, lookup);
    validator.declarations(&module.abi.public_items, &module.abi.trait_contracts)?;
    for declaration in &module.abi.native_declarations {
        validator.function(&declaration.function)?;
    }
    validator.layouts(&module.structures, &module.enumerations)?;
    for instance in &module.interface_instances {
        validator.types(&instance.arguments)?;
    }
    for function in &module.functions {
        validator.types(&function.instance.arguments)?;
        validator.slots(&function.semantic)?;
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            match instruction {
                Instruction::MapResultError { ty, .. }
                | Instruction::Iter { ty, .. }
                | Instruction::StandardEnum { ty, .. }
                | Instruction::MakeRange { ty, .. } => validator.validate_type(ty)?,
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
                    callee: CallTarget::Native(import),
                    ..
                } => {
                    validator.types(&import.instance.arguments)?;
                    validator.types(&import.signature.params)?;
                    validator.validate_type(&import.signature.result)?;
                    validator.bounds(&import.requirements)?;
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
