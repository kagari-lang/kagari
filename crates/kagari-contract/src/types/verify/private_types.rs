//! Module-owned private nominal evidence, separate from public export authority.

use crate::{
    layout::{EnumLayout, LayoutValidationError, StructLayout},
    types::PublicItem,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::{TypeDeclaration, TypeDef, TypeDefKind, verify::type_definition_valid},
    decode_limits::{MAX_NESTED_RECORDS, MAX_TABLE_RECORDS},
    ty::{Ty, substitution::TypeSubstitution},
};
use std::collections::{HashMap, HashSet};

/// Validate private declarations and every carried layout for their exact identities.
///
/// For an internal `Cell<Hidden>`, this retains `Hidden`'s declaration and bounds
/// without exposing it as a public path. Declaration shape, owner, ordered binders,
/// bounds and member types are checked here; linked application proofs remain with
/// the executable proof owner. A declaration need not have an executable layout,
/// but every layout that is present must agree after substitution.
///
/// Private names must be unique and disjoint from public type/trait names. This
/// routine never adds public items or treats external host schemas as local types.
/// Invalid authority/layout claims, resource limits and cancellation return errors.
pub fn validate(
    declarations: &[TypeDeclaration],
    public_items: &[PublicItem],
    module: &ModuleIdentity,
    structures: &[StructLayout],
    enumerations: &[EnumLayout],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), LayoutValidationError> {
    check_cancel(cancel)?;
    limit(declarations.len(), MAX_TABLE_RECORDS)?;
    let mut names = HashSet::new();
    for item in public_items {
        check_cancel(cancel)?;
        if matches!(item, PublicItem::Type(_) | PublicItem::Trait(_)) {
            names.insert(item.name());
        }
    }
    let mut private = HashMap::new();
    for declaration in declarations {
        check_cancel(cancel)?;
        let ty = &declaration.definition;
        declaration_limits(ty)?;
        declaration
            .visit_definition_ids(
                &mut |id| {
                    definitions.resolve(*id)?;
                    Ok(())
                },
                cancel,
            )
            .map_err(|error| match error {
                DefinitionMappingError::Cancelled => LayoutValidationError::Cancelled,
                DefinitionMappingError::LimitExceeded => LayoutValidationError::Limit {
                    resource: "private type shape",
                    limit: MAX_NESTED_RECORDS,
                },
                _ => LayoutValidationError::Invalid,
            })?;
        let valid = definitions
            .resolve(declaration.declaration)
            .is_ok_and(|view| view.module() == module)
            && names.insert(ty.name.as_str())
            && private.insert(declaration.declaration, ty).is_none()
            && type_definition_valid(ty, declaration.declaration, cancel, definitions);
        check_cancel(cancel)?;
        if !valid {
            return Err(LayoutValidationError::Invalid);
        }
    }
    for layout in structures {
        check_cancel(cancel)?;
        if let Some(ty) = private.get(&layout.declaration) {
            let valid = struct_matches(layout, ty, cancel, definitions);
            check_cancel(cancel)?;
            if !valid {
                return Err(LayoutValidationError::Invalid);
            }
        }
    }
    for layout in enumerations {
        check_cancel(cancel)?;
        if let Some(ty) = private.get(&layout.declaration) {
            let valid = enum_matches(layout, ty, cancel, definitions);
            check_cancel(cancel)?;
            if !valid {
                return Err(LayoutValidationError::Invalid);
            }
        }
    }
    Ok(())
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), LayoutValidationError> {
    cancel.check().map_err(|_| LayoutValidationError::Cancelled)
}

fn limit(count: usize, maximum: usize) -> Result<(), LayoutValidationError> {
    if count > maximum {
        return Err(LayoutValidationError::Limit {
            resource: "private type records",
            limit: maximum,
        });
    }
    Ok(())
}

fn declaration_limits(ty: &TypeDef) -> Result<(), LayoutValidationError> {
    for count in [
        ty.generic_params.len(),
        ty.bounds.len(),
        ty.fields.len(),
        ty.variants.len(),
    ] {
        limit(count, MAX_NESTED_RECORDS)?;
    }
    for bound in &ty.bounds {
        limit(bound.constraints.len(), MAX_NESTED_RECORDS)?;
    }
    for variant in &ty.variants {
        limit(variant.payload.len(), MAX_NESTED_RECORDS)?;
    }
    Ok(())
}

fn arguments_match(ty: &TypeDef, arguments: &[Ty]) -> bool {
    arguments.len() == ty.generic_params.len()
        && arguments.iter().all(Ty::within_wire_limits)
        && (arguments.iter().all(Ty::is_concrete)
            || arguments
                .iter()
                .zip(&ty.generic_params)
                .all(|(argument, parameter)| argument == &parameter.as_type()))
}

fn member_matches(
    member: DefinitionId,
    owner: DefinitionId,
    kind: DefinitionKind,
    name: &str,
    definitions: &DefinitionTable,
) -> bool {
    definitions.lookup_child(owner, kind, name, 0) == Some(member)
}

fn struct_matches(
    layout: &StructLayout,
    ty: &TypeDef,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> bool {
    if ty.kind != TypeDefKind::Struct
        || !arguments_match(ty, &layout.arguments)
        || layout.fields.len() != ty.fields.len()
    {
        return false;
    }
    let substitution = TypeSubstitution::for_owner(&layout.declaration, &layout.arguments);
    layout
        .fields
        .iter()
        .zip(&ty.fields)
        .all(|(field, declared)| {
            cancel.check().is_ok()
                && member_matches(
                    field.declaration,
                    layout.declaration,
                    DefinitionKind::Field,
                    &declared.name,
                    definitions,
                )
                && field.name == declared.name
                && field.visibility == declared.visibility
                && field.mutable == declared.mutable
                && substitution.apply(&declared.ty, cancel).ok().as_ref() == Some(&field.ty)
        })
}

fn enum_matches(
    layout: &EnumLayout,
    ty: &TypeDef,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> bool {
    if ty.kind != TypeDefKind::Enum
        || !arguments_match(ty, &layout.arguments)
        || layout.variants.len() != ty.variants.len()
    {
        return false;
    }
    let substitution = TypeSubstitution::for_owner(&layout.declaration, &layout.arguments);
    layout
        .variants
        .iter()
        .zip(&ty.variants)
        .all(|(variant, declared)| {
            cancel.check().is_ok()
                && member_matches(
                    variant.declaration,
                    layout.declaration,
                    DefinitionKind::Variant,
                    &declared.name,
                    definitions,
                )
                && variant.reports_failure == declared.reports_failure
                && variant.payload.len() == declared.payload.len()
                && variant
                    .payload
                    .iter()
                    .zip(&declared.payload)
                    .all(|(value, declared)| {
                        substitution.apply(declared, cancel).ok().as_ref() == Some(value)
                    })
        })
}
