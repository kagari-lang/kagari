use super::{Context, IrVerificationError, IrVerificationErrorKind as Error};
use crate::module::{
    IrModule,
    layout::{LayoutValidationError, validate_layouts},
};

pub(super) fn verify(module: &IrModule, context: Context<'_>) -> Result<(), IrVerificationError> {
    validate_layouts(&module.structures, context.cancel).map_err(|error| {
        context.error(match error {
            LayoutValidationError::Invalid => Error::InvalidStructLayout,
            LayoutValidationError::Limit { resource, limit } => Error::Limit { resource, limit },
            LayoutValidationError::Cancelled => Error::Cancelled,
        })
    })
}
