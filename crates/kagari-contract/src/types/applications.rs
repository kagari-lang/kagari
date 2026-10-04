//! Apply semantic declaration checks to a linked executable closure.
use crate::{
    layout::{EnumLayout, StructLayout},
    slots::SemanticSlots,
    types::{PublicItem, TraitContract},
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionPath};
use kagari_types::{
    declaration::{TraitDef, TypeDef, applications::ApplicationValidator},
    ty::substitution::TypeTransformError,
};

pub fn validate_declarations<'declaration, F, G>(
    validator: &ApplicationValidator<'_, F, G>,
    items: &[PublicItem],
    private: &[TraitContract],
    cancel: &CancellationToken,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionPath) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionPath) -> Option<&'declaration TypeDef>,
{
    for item in items {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        match item {
            PublicItem::Function(function) => validator.function(function)?,
            PublicItem::Const(value) => validator.validate_type(&value.ty)?,
            PublicItem::Type(record) => {
                validator.bounds(&record.bounds)?;
                validator.types(record.fields.iter().map(|field| &field.ty))?;
                validator.types(record.variants.iter().flat_map(|variant| &variant.payload))?;
            }
            PublicItem::Trait(record) => validator.trait_definition(record)?,
            PublicItem::InterfaceTable(table) => {
                validator.validate_type(&table.for_type)?;
                validator.validate_type(&table.trait_type)?;
                validator.bounds(&table.bounds)?;
                for method in &table.methods {
                    validator.function(method)?;
                }
                validator.types(table.associated_consts.iter().map(|member| &member.ty))?;
                for family in &table.associated_type_families {
                    validator.validate_type(&family.value)?;
                    validator.bounds(&family.bounds)?;
                }
            }
        }
    }
    for record in private {
        validator.trait_definition(&record.abi)?;
    }
    Ok(())
}

pub fn validate_layouts<'declaration, F, G>(
    validator: &ApplicationValidator<'_, F, G>,
    structures: &[StructLayout],
    enumerations: &[EnumLayout],
    _cancel: &CancellationToken,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionPath) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionPath) -> Option<&'declaration TypeDef>,
{
    for layout in structures {
        validator.types(&layout.arguments)?;
        validator.types(layout.fields.iter().map(|field| &field.ty))?;
    }
    for layout in enumerations {
        validator.types(&layout.arguments)?;
        validator.types(layout.variants.iter().flat_map(|variant| &variant.payload))?;
    }
    Ok(())
}

pub fn validate_slots<'declaration, F, G>(
    validator: &ApplicationValidator<'_, F, G>,
    slots: &SemanticSlots,
    _cancel: &CancellationToken,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionPath) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionPath) -> Option<&'declaration TypeDef>,
{
    if let Some(body) = &slots.generic {
        validator.bounds(&body.bounds)?;
    }
    if let Some(required) = &slots.protocol_adapter {
        validator.validate_type(&required.receiver)?;
        validator.trait_application_contract(&required.interface)?;
        validator.types(&required.arguments)?;
    }
    validator.types(
        slots
            .params
            .values()
            .chain(slots.result.iter())
            .chain(slots.locals.values())
            .chain(slots.registers.values()),
    )
}
#[cfg(test)]
mod tests;
