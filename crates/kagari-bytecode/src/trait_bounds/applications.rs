use crate::{
    instruction::{BytecodeInstruction, CallTarget, ConstructionTarget as BytecodeConstruction},
    module::BytecodeModule,
    trait_bounds::contract,
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionTable};
use kagari_contract::{
    callable::witness::OperationWitness,
    types::{
        applications::{
            validate_declarations, validate_function, validate_layouts, validate_slots,
            validate_types,
        },
        named::origins,
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
    definitions: &DefinitionTable,
) -> Result<(), TypeTransformError> {
    origins::validate(
        &module.function_origins,
        &module.identity,
        definitions,
        cancel,
        module.functions.iter().filter_map(|function| {
            Some((function.identity.as_ref()?, &function.metadata.semantic))
        }),
        |id| {
            closure
                .iter()
                .flat_map(|module| &module.named_declarations)
                .find(|record| record.declaration == id)
        },
    )?;
    let validator = ApplicationValidator::new(
        cancel,
        |id| contract(id, closure, definitions),
        |id| {
            closure.iter().find_map(|owner| {
                type_contract(&owner.identity, &owner.public_items, id, definitions)
                    .or_else(|| {
                        owner
                            .private_types
                            .iter()
                            .find(|record| &record.declaration == id)
                            .map(|record| &record.definition)
                    })
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
        &module.private_types,
        &module.named_declarations,
        cancel,
        definitions,
    )?;
    for declaration in &module.native_declarations {
        validate_function(&validator, &declaration.function, cancel, definitions)?;
        for required in &declaration.callable_requirements {
            validator.validate_type(&required.receiver)?;
            validator.trait_application(&required.interface)?;
            validator.types(&required.arguments)?;
        }
    }
    for import in &module.native_imports {
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
        validate_slots(&validator, &function.metadata.semantic, cancel, definitions)?;
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
                BytecodeInstruction::MakeInterface { target, .. } => match target {
                    BytecodeConstruction::Declared { arguments, .. } => {
                        validator.types(arguments)?
                    }
                    BytecodeConstruction::Bound(required) => {
                        validator.validate_type(&required.receiver)?;
                        validator.trait_application(&required.interface)?;
                    }
                },
                BytecodeInstruction::MakeArray { element, .. }
                | BytecodeInstruction::RepeatArray { element, .. } => {
                    validator.validate_type(element)?
                }
                BytecodeInstruction::MakeFuture { future: ty, .. }
                | BytecodeInstruction::Await { future: ty, .. }
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
