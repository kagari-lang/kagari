use super::{Context, IrVerificationError, IrVerificationErrorKind as Error};
use crate::module::{
    IrModule,
    layout::{LayoutValidationError, validate_layouts},
};

pub(super) fn verify(module: &IrModule, context: Context<'_>) -> Result<(), IrVerificationError> {
    let mut functions = std::collections::BTreeMap::new();
    for instruction in module
        .functions
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.instructions)
    {
        context
            .cancel
            .check()
            .map_err(|_| context.error(Error::Cancelled))?;
        if let crate::module::Instruction::Call {
            callee: crate::module::CallTarget::HostFunction(function),
            ..
        } = instruction
            && functions
                .insert(function.id.clone(), function.as_ref().clone())
                .is_some_and(|previous| !previous.matches_binding(function))
        {
            return Err(context.error(Error::InvalidHostInterface));
        }
    }
    crate::module::host::validate(
        &kagari_common::host_interface::HostInterface {
            field_paths: vec![],
            types: module.host_types.clone(),
            functions: functions.into_values().collect(),
        },
        &module.abi.public_items,
        &module.structures,
        &module.enumerations,
        context.cancel,
    )
    .map_err(|error| {
        context.error(match error {
            LayoutValidationError::Cancelled => Error::Cancelled,
            _ => Error::InvalidHostInterface,
        })
    })?;
    crate::module::abi::verify::validate(
        &module.abi.public_items,
        &module.identity,
        context.cancel,
    )
    .map_err(|error| {
        context.error(match error {
            LayoutValidationError::Cancelled => Error::Cancelled,
            _ => Error::InvalidPublicAbi,
        })
    })?;
    if !crate::module::layout::enum_abi_matches(
        &module.enumerations,
        &module.identity,
        &module.abi.public_items,
    ) {
        return Err(context.error(Error::InvalidEnumLayout));
    }
    validate_layouts(&module.structures, context.cancel).map_err(|error| {
        context.error(match error {
            LayoutValidationError::Invalid => Error::InvalidStructLayout,
            LayoutValidationError::Limit { resource, limit } => Error::Limit { resource, limit },
            LayoutValidationError::Cancelled => Error::Cancelled,
        })
    })?;
    crate::module::layout::validate_enum_layouts(
        &module.enumerations,
        &module.structures,
        context.cancel,
    )
    .map_err(|error| {
        context.error(match error {
            LayoutValidationError::Invalid => Error::InvalidEnumLayout,
            LayoutValidationError::Limit { resource, limit } => Error::Limit { resource, limit },
            LayoutValidationError::Cancelled => Error::Cancelled,
        })
    })
}
