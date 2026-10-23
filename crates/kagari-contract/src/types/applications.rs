//! Apply semantic declaration checks to a linked executable closure.
use crate::{
    layout::{EnumLayout, StructLayout},
    slots::FunctionTypeFacts,
    types::{PublicItem, TraitContract},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::{
        FnDecl, TraitDef, TypeDeclaration, TypeDef, TypeDefKind, applications::ApplicationValidator,
    },
    ty::{Ty, substitution::TypeTransformError},
};

mod dynamic;

pub fn validate_declarations<'declaration, F, G>(
    validator: &ApplicationValidator<'_, F, G>,
    items: &[PublicItem],
    private: &[TraitContract],
    private_types: &[TypeDeclaration],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionId) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionId) -> Option<(TypeDefKind, usize)>,
{
    for item in items {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        match item {
            PublicItem::Function(function) => {
                validate_function(validator, function, cancel, definitions)?
            }
            PublicItem::Static(value) => {
                validate_types(validator, [&value.ty], cancel, definitions)?
            }
            PublicItem::Const(value) => {
                validate_types(validator, [&value.ty], cancel, definitions)?
            }
            PublicItem::Type(record) => {
                validate_type_definition(validator, record, cancel, definitions)?
            }
            PublicItem::Trait(record) => validate_trait(validator, record, cancel, definitions)?,
            PublicItem::InherentTable(table) => {
                validate_types(validator, [&table.for_type], cancel, definitions)?;
                validator.bounds(&table.bounds)?;
                for method in &table.methods {
                    validate_function(validator, method, cancel, definitions)?;
                }
            }
            PublicItem::InterfaceTable(table) => {
                validate_types(validator, [&table.for_type], cancel, definitions)?;
                validator.validate_type(&table.trait_type)?;
                validator.bounds(&table.bounds)?;
                for method in &table.methods {
                    validate_function(validator, method, cancel, definitions)?;
                }
                validate_types(
                    validator,
                    table.associated_consts.iter().map(|member| &member.ty),
                    cancel,
                    definitions,
                )?;
                for family in &table.associated_type_families {
                    validate_types(validator, [&family.value], cancel, definitions)?;
                    validator.bounds(&family.bounds)?;
                }
            }
        }
    }
    for record in private_types {
        validate_type_definition(validator, &record.definition, cancel, definitions)?;
    }
    for record in private {
        validate_trait(validator, &record.abi, cancel, definitions)?;
    }
    Ok(())
}

fn validate_type_definition<'declaration, F, G>(
    validator: &ApplicationValidator<'_, F, G>,
    record: &TypeDef,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionId) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionId) -> Option<(TypeDefKind, usize)>,
{
    validator.bounds(&record.bounds)?;
    validate_types(
        validator,
        record
            .fields
            .iter()
            .map(|field| &field.ty)
            .chain(record.variants.iter().flat_map(|variant| &variant.payload)),
        cancel,
        definitions,
    )
}

/// Check actual value types; trait applications in bounds and impl headers use
/// the declaration validator directly and need not be dynamically compatible.
pub fn validate_types<'ty, 'declaration, F, G>(
    validator: &ApplicationValidator<'_, F, G>,
    types: impl IntoIterator<Item = &'ty Ty>,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionId) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionId) -> Option<(TypeDefKind, usize)>,
{
    for ty in types {
        validator.validate_type(ty)?;
        dynamic::validate(
            [ty],
            &|id| validator.trait_declaration(id),
            definitions,
            cancel,
        )?;
    }
    Ok(())
}

pub fn validate_function<'declaration, F, G>(
    validator: &ApplicationValidator<'_, F, G>,
    function: &FnDecl,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionId) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionId) -> Option<(TypeDefKind, usize)>,
{
    validator.function(function)?;
    dynamic::validate(
        function
            .params
            .iter()
            .map(|param| &param.ty)
            .chain([&function.return_type]),
        &|id| validator.trait_declaration(id),
        definitions,
        cancel,
    )
}

fn validate_trait<'declaration, F, G>(
    validator: &ApplicationValidator<'_, F, G>,
    record: &TraitDef,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionId) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionId) -> Option<(TypeDefKind, usize)>,
{
    validator.trait_definition(record)?;
    for method in &record.methods {
        validate_function(validator, method, cancel, definitions)?;
    }
    validate_types(
        validator,
        record.associated_consts.iter().map(|member| &member.ty),
        cancel,
        definitions,
    )
}

pub fn validate_layouts<'declaration, F, G>(
    validator: &ApplicationValidator<'_, F, G>,
    structures: &[StructLayout],
    enumerations: &[EnumLayout],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionId) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionId) -> Option<(TypeDefKind, usize)>,
{
    dynamic::validate(
        structures
            .iter()
            .flat_map(|layout| layout.fields.iter().map(|field| &field.ty))
            .chain(
                enumerations
                    .iter()
                    .flat_map(|layout| layout.variants.iter().flat_map(|variant| &variant.payload)),
            ),
        &|id| validator.trait_declaration(id),
        definitions,
        cancel,
    )?;
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
    slots: &impl FunctionTypeFacts,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<(), TypeTransformError>
where
    F: Fn(&DefinitionId) -> Option<&'declaration TraitDef>,
    G: Fn(&DefinitionId) -> Option<(TypeDefKind, usize)>,
{
    if let Some(body) = slots.generic() {
        validator.bounds(&body.bounds)?;
    }
    if let Some(required) = slots.protocol_adapter() {
        validator.validate_type(&required.receiver)?;
        validator.trait_application_contract(&required.interface)?;
        validator.types(&required.arguments)?;
    }
    dynamic::validate(
        slots
            .params()
            .values()
            .chain(slots.result())
            .chain(slots.value_types()),
        &|id| validator.trait_declaration(id),
        definitions,
        cancel,
    )?;
    validator.types(
        slots
            .params()
            .values()
            .chain(slots.result())
            .chain(slots.value_types()),
    )
}
#[cfg(test)]
mod tests;
