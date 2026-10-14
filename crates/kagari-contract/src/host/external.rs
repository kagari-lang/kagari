//! External dependencies retain the original common type schema and identity.
//! Local validation proves shape; linked validation proves the defining provider.
use crate::{
    layout::LayoutValidationError,
    types::{PublicItem, verify},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::declaration::{TypeDeclaration, TypeDef, TypeDefKind};
use std::{collections::BTreeSet, slice};

/// Check the same TypeDef schema validator used by public declarations, including
/// private field policy. Reject duplicate, foreign, nonexternal and renamed facts.
pub fn validate(
    types: &[TypeDeclaration],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), LayoutValidationError> {
    let mut ids = BTreeSet::new();
    for ty in types {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let view = definitions
            .resolve(ty.declaration)
            .map_err(|_| LayoutValidationError::Invalid)?;
        if !ids.insert(ty.declaration)
            || !matches!(ty.definition.kind, TypeDefKind::Host(_))
            || !view.last().is_some_and(|part| {
                part.kind == DefinitionKind::Struct
                    && part.name == ty.definition.name
                    && part.occurrence == 0
            })
            || definitions.lookup_root(view.module()).and_then(|root| {
                definitions.lookup_child(root, DefinitionKind::Struct, &ty.definition.name, 0)
            }) != Some(ty.declaration)
        {
            return Err(LayoutValidationError::Invalid);
        }
        verify::validate(
            slice::from_ref(&PublicItem::Type(ty.definition.clone())),
            view.module(),
            cancel,
            definitions,
        )?;
    }
    Ok(())
}

/// Require each carried schema to equal its reachable defining module's common
/// declaration. A dependency copy cannot authorize a different field or access.
pub fn bindings_match<'declaration>(
    types: &[TypeDeclaration],
    provider: impl Fn(DefinitionId) -> Option<&'declaration TypeDef>,
    cancel: &CancellationToken,
) -> Result<bool, LayoutValidationError> {
    for ty in types {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        if provider(ty.declaration) != Some(&ty.definition) {
            return Ok(false);
        }
    }
    Ok(true)
}
