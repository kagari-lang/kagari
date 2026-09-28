use crate::{
    CallTarget, Instruction, MirModule,
    verify::{Context, MirVerificationError, MirVerificationErrorKind as Error},
};
use kagari_abi::{
    host,
    layout::{self, LayoutValidationError, validate_layouts},
    types::verify as abi_verify,
};
use kagari_common::host_interface::HostInterface;
use std::collections::BTreeMap;

pub(super) fn verify(module: &MirModule, context: Context<'_>) -> Result<(), MirVerificationError> {
    let mut functions = BTreeMap::new();
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
        if let Instruction::Call {
            callee: CallTarget::HostFunction(function),
            ..
        } = instruction
            && functions
                .insert(function.id.clone(), function.as_ref().clone())
                .is_some_and(|previous| !previous.matches_binding(function))
        {
            return Err(context.error(Error::InvalidHostInterface));
        }
    }
    host::validate(
        &HostInterface {
            paths: vec![],
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
    abi_verify::validate(&module.abi.public_items, &module.identity, context.cancel).map_err(
        |error| {
            context.error(match error {
                LayoutValidationError::Cancelled => Error::Cancelled,
                _ => Error::InvalidPublicAbi,
            })
        },
    )?;
    abi_verify::validate_trait_contracts(
        &module.abi.trait_contracts,
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
    if !host::trait_bindings_match(
        &HostInterface {
            types: module.host_types.clone(),
            ..Default::default()
        },
        &module.identity,
        &module.abi.public_items,
        &module.abi.trait_contracts,
        context.cancel,
    )
    .map_err(|_| context.error(Error::Cancelled))?
    {
        return Err(context.error(Error::InvalidHostInterface));
    }
    if !layout::struct_abi_matches(
        &module.structures,
        &module.identity,
        &module.abi.public_items,
        context.cancel,
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
    )
    .map_err(|_| context.error(Error::Cancelled))?
    {
        return Err(context.error(Error::InvalidEnumLayout));
    }
    validate_layouts(&module.structures, context.cancel).map_err(|error| {
        context.error(match error {
            LayoutValidationError::Invalid => Error::InvalidStructLayout,
            LayoutValidationError::Limit { resource, limit } => Error::Limit { resource, limit },
            LayoutValidationError::Cancelled => Error::Cancelled,
        })
    })?;
    layout::validate_enum_layouts(&module.enumerations, &module.structures, context.cancel).map_err(
        |error| {
            context.error(match error {
                LayoutValidationError::Invalid => Error::InvalidEnumLayout,
                LayoutValidationError::Limit { resource, limit } => {
                    Error::Limit { resource, limit }
                }
                LayoutValidationError::Cancelled => Error::Cancelled,
            })
        },
    )
}
