//! Nominal host dependencies include signatures and layouts, even without a call.
use super::{EnumLayout, PublicAbiItem, StructLayout, abi::AbiType};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::DefinitionId,
};
use std::collections::BTreeSet;

pub(crate) fn references(
    items: &[PublicAbiItem],
    structures: &[StructLayout],
    enums: &[EnumLayout],
    cancel: &CancellationToken,
) -> Result<BTreeSet<DefinitionId>, Cancelled> {
    let mut pending = Vec::new();
    for item in items {
        cancel.check()?;
        let functions: &[super::FunctionAbi] = match item {
            PublicAbiItem::Function(function) => std::slice::from_ref(function),
            PublicAbiItem::Const(value) => {
                pending.push(&value.ty);
                &[]
            }
            PublicAbiItem::Type(ty) => {
                pending.extend(ty.fields.iter().map(|field| &field.ty));
                pending.extend(ty.variants.iter().flat_map(|variant| &variant.payload));
                &[]
            }
            PublicAbiItem::Trait(ty) => &ty.methods,
            PublicAbiItem::InterfaceTable(table) => {
                pending.extend([&table.trait_type, &table.for_type]);
                &table.methods
            }
        };
        for function in functions {
            pending.extend(function.params.iter().map(|param| &param.ty));
            pending.push(&function.return_type);
        }
    }
    pending.extend(structures.iter().flat_map(|layout| &layout.arguments));
    for layout in enums {
        pending.extend(&layout.arguments);
        pending.extend(layout.variants.iter().flat_map(|variant| &variant.payload));
    }
    let mut result = BTreeSet::new();
    while let Some(ty) = pending.pop() {
        cancel.check()?;
        match ty {
            AbiType::Host(id) => {
                result.insert(id.clone());
            }
            AbiType::Tuple(types) | AbiType::StandardEnum { args: types, .. } => {
                pending.extend(types)
            }
            AbiType::Array(ty) | AbiType::Set(ty) => pending.push(ty),
            AbiType::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
            AbiType::Struct(ty) | AbiType::Enum(ty) | AbiType::Trait(ty) => {
                pending.extend(&ty.arguments)
            }
            _ => {}
        }
    }
    Ok(result)
}

pub(crate) fn validate(
    interface: &kagari_common::host_interface::HostInterface,
    items: &[PublicAbiItem],
    structures: &[StructLayout],
    enums: &[EnumLayout],
    cancel: &CancellationToken,
) -> Result<(), super::layout::LayoutValidationError> {
    use super::layout::LayoutValidationError as Error;
    cancel.check().map_err(|_| Error::Cancelled)?;
    interface.validate().map_err(|_| Error::Invalid)?;
    let ids: BTreeSet<_> = interface.types.iter().map(|ty| &ty.id).collect();
    if references(items, structures, enums, cancel)
        .map_err(|_| Error::Cancelled)?
        .iter()
        .all(|id| ids.contains(id))
    {
        Ok(())
    } else {
        Err(Error::Invalid)
    }
}
