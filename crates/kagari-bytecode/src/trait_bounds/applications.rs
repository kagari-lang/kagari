use crate::{
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeModule,
    trait_bounds::contract,
};
use kagari_abi::types::{applications::ApplicationValidator, substitution::TypeTransformError};
use kagari_common::cancellation::CancellationToken;

pub(super) fn validate(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<(), TypeTransformError> {
    let validator = ApplicationValidator::new(cancel, |id| contract(id, closure));
    validator.declarations(&module.public_items, &module.trait_contracts)?;
    for declaration in &module.native_declarations {
        validator.function(&declaration.function)?;
    }
    for import in &module.native_imports {
        validator.types(&import.instance.arguments)?;
        validator.types(&import.signature.params)?;
        validator.validate_type(&import.signature.result)?;
        validator.bounds(&import.requirements)?;
    }
    validator.layouts(&module.structures, &module.enumerations)?;
    for table in &module.interface_tables {
        validator.types(&table.arguments)?;
    }
    for record in &module.function_table {
        if let Some(identity) = &record.identity {
            validator.types(&identity.arguments)?;
        }
    }
    for function in &module.functions {
        if let Some(identity) = &function.identity {
            validator.types(&identity.arguments)?;
        }
        validator.slots(&function.metadata.semantic)?;
        for instruction in &function.instructions {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            match instruction {
                BytecodeInstruction::MapResultError { ty, .. }
                | BytecodeInstruction::Iter { ty, .. }
                | BytecodeInstruction::StandardEnum { ty, .. }
                | BytecodeInstruction::MakeRange { ty, .. } => validator.validate_type(ty)?,
                BytecodeInstruction::RangeBound { range, bound, .. } => {
                    validator.types([range, bound])?
                }
                BytecodeInstruction::UpcastInterface { source, target, .. } => {
                    validator.trait_application(source)?;
                    validator.trait_application(target)?;
                }
                BytecodeInstruction::Call {
                    callee: CallTarget::InterfaceMethod { interface, .. },
                    ..
                } => validator.trait_application(interface)?,
                _ => {}
            }
        }
    }
    Ok(())
}
