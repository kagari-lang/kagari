use crate::{
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeModule,
    trait_bounds::contract,
};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::{
    callable::witness::OperationWitness,
    types::{
        applications::{validate_declarations, validate_layouts, validate_slots},
        type_contract,
    },
};
use kagari_types::{
    declaration::{TypeDefKind, applications::ApplicationValidator},
    ty::substitution::TypeTransformError,
};

pub(super) fn validate(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<(), TypeTransformError> {
    let validator = ApplicationValidator::new(
        cancel,
        |id| contract(id, closure),
        |id| {
            closure.iter().find_map(|owner| {
                type_contract(&owner.identity, &owner.public_items, id)
                    .map(|record| (record.kind, record.generic_params.len()))
                    .or_else(|| {
                        owner
                            .enumerations
                            .iter()
                            .find(|layout| &layout.declaration == id)
                            .map(|layout| (TypeDefKind::Enum, layout.arguments.len()))
                    })
                    .or_else(|| {
                        owner
                            .structures
                            .iter()
                            .find(|layout| &layout.declaration == id)
                            .map(|layout| (TypeDefKind::Struct, layout.arguments.len()))
                    })
            })
        },
    );
    validate_declarations(
        &validator,
        &module.public_items,
        &module.trait_contracts,
        cancel,
    )?;
    for declaration in &module.native_declarations {
        validator.function(&declaration.function)?;
        for required in &declaration.callable_requirements {
            validator.validate_type(&required.receiver)?;
            validator.trait_application(&required.interface)?;
            validator.types(&required.arguments)?;
        }
    }
    for import in &module.native_imports {
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
    for table in &module.interface_tables {
        validator.types(&table.arguments)?;
        for slot in &table.methods {
            validator.types(&slot.arguments)?;
        }
        for parent in &table.parents {
            validator.trait_application(&parent.interface)?;
            validator.types(&parent.implementation.arguments)?;
        }
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
        validate_slots(&validator, &function.metadata.semantic, cancel)?;
        for instruction in &function.instructions {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if let Some(arguments) = instruction.layout_arguments() {
                validator.types(arguments)?;
            }
            match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::Shared { contract, .. },
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
                BytecodeInstruction::MakeInterface { arguments, .. } => {
                    validator.types(arguments)?
                }
                BytecodeInstruction::MakeArray { element, .. }
                | BytecodeInstruction::RepeatArray { element, .. } => {
                    validator.validate_type(element)?
                }
                BytecodeInstruction::Await { future: ty, .. }
                | BytecodeInstruction::Iter { ty, .. }
                | BytecodeInstruction::MakeRange { ty, .. } => validator.validate_type(ty)?,
                BytecodeInstruction::RangeBound { range, bound, .. } => {
                    validator.types([range, bound])?
                }
                BytecodeInstruction::UpcastInterface { source, target, .. } => {
                    validator.trait_application(source)?;
                    validator.trait_application(target)?;
                }
                BytecodeInstruction::Call {
                    callee: CallTarget::InterfaceMethod { contract, .. },
                    ..
                } => {
                    validator.trait_application(&contract.interface)?;
                    validator.types(&contract.arguments)?;
                    for fact in &contract.normalizations {
                        validator.validate_type(&fact.source)?;
                        validator.validate_type(&fact.result)?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
