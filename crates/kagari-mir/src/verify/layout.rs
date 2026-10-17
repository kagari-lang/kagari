use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction},
    verify::{Context, MirVerificationError, MirVerificationErrorKind as Error},
};
use kagari_contract::{
    host, layout,
    layout::{LayoutValidationError, validate_layouts},
    types::{inherent::native_signatures_match, verify as abi_verify},
};
use kagari_types::declaration::verify::validate_native_declarations;
use std::iter;

pub(super) fn verify(module: &MirModule, context: Context<'_>) -> Result<(), MirVerificationError> {
    host::validate(
        &module.external_types,
        &module.abi.public_items,
        &module.structures,
        &module.enumerations,
        context.cancel,
        context.definitions,
    )
    .map_err(|error| {
        context.error(match error {
            LayoutValidationError::Cancelled => Error::Cancelled,
            _ => Error::InvalidHostInterface,
        })
    })?;
    if !host::references_present(
        &module.external_types,
        module
            .abi
            .native_declarations
            .iter()
            .flat_map(|declaration| {
                declaration
                    .function
                    .params
                    .iter()
                    .map(|param| &param.ty)
                    .chain(iter::once(&declaration.function.return_type))
            })
            .chain(module.native_targets.iter().flat_map(|import| {
                import
                    .signature
                    .params
                    .iter()
                    .chain(iter::once(&import.signature.result))
            }))
            .chain(module.functions.iter().flat_map(|function| {
                function
                    .semantic
                    .params
                    .values()
                    .chain(function.semantic.result.iter())
                    .chain(function.semantic.locals.values())
                    .chain(function.semantic.registers.values())
            }))
            .chain(
                module
                    .functions
                    .iter()
                    .flat_map(|function| &function.blocks)
                    .flat_map(|block| &block.instructions)
                    .filter_map(|instruction| match instruction {
                        Instruction::Call {
                            callee: CallTarget::Native(import),
                            ..
                        } => Some(import),
                        _ => None,
                    })
                    .flat_map(|import| {
                        import
                            .signature
                            .params
                            .iter()
                            .chain(iter::once(&import.signature.result))
                    }),
            ),
        context.cancel,
    )
    .map_err(|_| context.error(Error::Cancelled))?
    {
        return Err(context.error(Error::InvalidHostInterface));
    }
    abi_verify::validate(
        &module.abi.public_items,
        &module.identity,
        context.cancel,
        context.definitions,
    )
    .map_err(|error| {
        context.error(match error {
            LayoutValidationError::Cancelled => Error::Cancelled,
            _ => Error::InvalidPublicAbi,
        })
    })?;
    validate_native_declarations(
        &module.abi.native_declarations,
        &module.identity,
        context.definitions,
        context.cancel,
    )
    .map_err(|_| context.error(Error::InvalidPublicAbi))?;
    if !native_signatures_match(
        &module.abi.public_items,
        &module.abi.native_declarations,
        context.definitions,
    ) {
        return Err(context.error(Error::InvalidPublicAbi));
    }
    abi_verify::validate_trait_references(
        &module.abi.trait_references,
        &module.identity,
        context.cancel,
        context.definitions,
    )
    .map_err(|error| {
        context.error(match error {
            LayoutValidationError::Cancelled => Error::Cancelled,
            _ => Error::InvalidPublicAbi,
        })
    })?;
    abi_verify::validate_trait_contracts(
        &module.abi.trait_contracts,
        &module.abi.public_items,
        &module.identity,
        context.cancel,
        context.definitions,
    )
    .map_err(|error| {
        context.error(match error {
            LayoutValidationError::Cancelled => Error::Cancelled,
            _ => Error::InvalidPublicAbi,
        })
    })?;

    if !layout::struct_abi_matches(
        &module.structures,
        &module.identity,
        &module.abi.public_items,
        context.cancel,
        context.definitions,
    )
    .map_err(|_| context.error(Error::Cancelled))?
    {
        return Err(context.error(Error::InvalidStructLayout));
    }
    if !layout::enum_abi_matches(
        &module.enumerations,
        &module.identity,
        &module.abi.public_items,
        context.cancel,
        context.definitions,
    )
    .map_err(|_| context.error(Error::Cancelled))?
    {
        return Err(context.error(Error::InvalidEnumLayout));
    }
    validate_layouts(&module.structures, context.cancel, context.definitions).map_err(|error| {
        context.error(match error {
            LayoutValidationError::Invalid => Error::InvalidStructLayout,
            LayoutValidationError::Limit { resource, limit } => Error::Limit { resource, limit },
            LayoutValidationError::Cancelled => Error::Cancelled,
        })
    })?;
    layout::validate_enum_layouts(
        &module.enumerations,
        &module.structures,
        context.cancel,
        context.definitions,
    )
    .map_err(|error| {
        context.error(match error {
            LayoutValidationError::Invalid => Error::InvalidEnumLayout,
            LayoutValidationError::Limit { resource, limit } => Error::Limit { resource, limit },
            LayoutValidationError::Cancelled => Error::Cancelled,
        })
    })
}
