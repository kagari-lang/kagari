//! Nominal host dependencies include signatures and layouts, even without a call.

use crate::{
    layout::{EnumLayout, LayoutValidationError, StructLayout},
    types::PublicItem,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::{FnDecl, TypeDeclaration},
    ty::Ty,
};
use std::{collections::BTreeSet, slice};

pub mod external;

pub fn references(
    items: &[PublicItem],
    structures: &[StructLayout],
    enums: &[EnumLayout],
    cancel: &CancellationToken,
) -> Result<BTreeSet<DefinitionId>, Cancelled> {
    let mut pending = Vec::new();
    for item in items {
        cancel.check()?;
        let functions: &[FnDecl] = match item {
            PublicItem::Function(function) => slice::from_ref(function),
            PublicItem::Const(value) => {
                pending.push(&value.ty);
                &[]
            }
            PublicItem::Type(ty) => {
                pending.extend(ty.fields.iter().map(|field| &field.ty));
                pending.extend(ty.variants.iter().flat_map(|variant| &variant.payload));
                &[]
            }
            PublicItem::Trait(ty) => &ty.methods,
            PublicItem::InherentTable(table) => {
                pending.push(&table.for_type);
                &table.methods
            }
            PublicItem::InterfaceTable(table) => {
                pending.extend([&table.trait_type, &table.for_type]);
                &table.methods
            }
        };
        for function in functions {
            pending.extend(function.params.iter().map(|param| &param.ty));
            pending.push(&function.return_type);
        }
    }
    for layout in structures {
        pending.extend(&layout.arguments);
        pending.extend(layout.fields.iter().map(|field| &field.ty));
    }
    for layout in enums {
        pending.extend(&layout.arguments);
        pending.extend(layout.variants.iter().flat_map(|variant| &variant.payload));
    }
    type_references(pending, cancel)
}

/// Collect external identities recursively without treating ordinary DTO nominals
/// as external storage. Cancellation is checked throughout the traversal.
pub fn type_references<'ty>(
    types: impl IntoIterator<Item = &'ty Ty>,
    cancel: &CancellationToken,
) -> Result<BTreeSet<DefinitionId>, Cancelled> {
    let mut pending: Vec<_> = types.into_iter().collect();
    let mut result = BTreeSet::new();
    while let Some(ty) = pending.pop() {
        cancel.check()?;
        match ty {
            Ty::Host(id) => {
                result.insert(*id);
            }
            Ty::Tuple(types) => pending.extend(types),
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            Ty::Array(ty) | Ty::Set(ty, _) | Ty::Iter(ty) | Ty::Reference { target: ty, .. } => {
                pending.push(ty)
            }
            Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            Ty::NativeObject(ty) | Ty::Struct(ty) | Ty::Enum(ty) | Ty::Trait(ty) => {
                pending.extend(&ty.arguments);
                pending.extend(ty.associated_types.values());
            }
            _ => {}
        }
    }
    Ok(result)
}

pub fn validate(
    external_types: &[TypeDeclaration],
    items: &[PublicItem],
    structures: &[StructLayout],
    enums: &[EnumLayout],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), LayoutValidationError> {
    cancel
        .check()
        .map_err(|_| LayoutValidationError::Cancelled)?;
    let mut ids = BTreeSet::new();
    external::validate(external_types, cancel, definitions)?;
    if external_types.iter().any(|ty| !ids.insert(ty.declaration)) {
        return Err(LayoutValidationError::Invalid);
    }
    let mut required = references(items, structures, enums, cancel)
        .map_err(|_| LayoutValidationError::Cancelled)?;
    required.extend(
        type_references(
            external_types
                .iter()
                .flat_map(|ty| ty.definition.fields.iter().map(|field| &field.ty)),
            cancel,
        )
        .map_err(|_| LayoutValidationError::Cancelled)?,
    );
    if required.iter().all(|id| ids.contains(id)) {
        Ok(())
    } else {
        Err(LayoutValidationError::Invalid)
    }
}

/// Require executable semantic types to use a declared external dependency.
/// Callers validate dependency schemas first; the linked provider check follows.
pub fn references_present<'ty>(
    external_types: &[TypeDeclaration],
    types: impl IntoIterator<Item = &'ty Ty>,
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    let ids: BTreeSet<_> = external_types.iter().map(|ty| ty.declaration).collect();
    Ok(type_references(types, cancel)?
        .iter()
        .all(|id| ids.contains(id)))
}
