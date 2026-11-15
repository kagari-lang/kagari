//! Structural correspondence for explicitly supplied, validated tool source.
use crate::{hir::item::Item, lower::LoweredModule, native::structured::RegistrationMap};
use kagari_common::identity::{DefinitionKind, table::DefinitionTable};
use kagari_types::declaration::{
    TypeDefKind,
    module::{DeclarationError, ModuleDecl},
};

/// The exact non-trivia projection has already established declaration order and
/// complete Script bodies. Bind typed structural slots, never names or byte spans.
/// Default registration instead records these handles during direct allocation.
pub(super) fn supplied(
    declaration: &ModuleDecl,
    definitions: &DefinitionTable,
    lowered: &LoweredModule,
) -> Result<RegistrationMap, DeclarationError> {
    let invalid = || DeclarationError("registered structure differs from supplied source".into());
    let mut map = RegistrationMap::default();
    let mut items = lowered.module.items.iter().copied();
    for ty in &declaration.types {
        let item = items.next().ok_or_else(invalid)?;
        let matching_kind = match ty.kind {
            TypeDefKind::Struct => matches!(item, Item::Struct(_)),
            TypeDefKind::Enum => matches!(item, Item::Enum(_)),
            _ => matches!(item, Item::NativeType(_)),
        };
        if !matching_kind {
            return Err(invalid());
        }
        let owner = declaration.definition(ty.kind.definition_kind(), &ty.name, definitions)?;
        map.items.insert(owner, item);
    }
    for contract in &declaration.traits {
        let Some(Item::Trait(id)) = items.next() else {
            return Err(invalid());
        };
        let item = &lowered.module.traits[id.index()];
        if item.methods.len() != contract.methods.len() {
            return Err(invalid());
        }
        let owner = declaration.definition(DefinitionKind::Trait, &contract.name, definitions)?;
        map.items.insert(owner, Item::Trait(id));
        for (member, method) in item.methods.iter().zip(&contract.methods) {
            map.items.insert(
                ModuleDecl::method_id(owner, &method.name, definitions)?,
                Item::Function(member.function),
            );
        }
    }
    for (index, implementation) in declaration.implementations.iter().enumerate() {
        let Some(Item::Impl(id)) = items.next() else {
            return Err(invalid());
        };
        let item = &lowered.module.impls[id.index()];
        if item.methods.len() != implementation.methods.len() {
            return Err(invalid());
        }
        let owner = declaration.implementation_id(index, definitions)?;
        map.items.insert(owner, Item::Impl(id));
        for (member, method) in item.methods.iter().zip(&implementation.methods) {
            map.items.insert(
                ModuleDecl::method_id(owner, &method.name, definitions)?,
                Item::Function(member.function),
            );
        }
    }
    for function in &declaration.functions {
        let Some(Item::Function(id)) = items.next() else {
            return Err(invalid());
        };
        map.items.insert(
            declaration.definition(DefinitionKind::Function, &function.name, definitions)?,
            Item::Function(id),
        );
    }
    if items.next().is_some() {
        return Err(invalid());
    }
    Ok(map)
}
