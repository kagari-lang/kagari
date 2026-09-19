use super::{Context, IrVerificationError, IrVerificationErrorKind as Error};
use crate::module::{
    IrModule,
    layout::{LayoutValidationError, validate_layouts},
};

pub(super) fn verify(module: &IrModule, context: Context<'_>) -> Result<(), IrVerificationError> {
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
