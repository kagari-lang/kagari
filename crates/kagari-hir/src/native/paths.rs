//! Source package spelling and inherent ownership from the installed declarations.
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeStorageLayout,
    },
    ty::Ty,
};
use std::collections::BTreeMap;

pub(crate) fn module_path<'a>(
    identity: &ModuleIdentity,
    mut providers: impl Iterator<Item = &'a ModuleDecl>,
) -> String {
    let package = providers
        .find(|module| module.identity == *identity)
        .and_then(|module| module.package_alias.as_deref())
        .unwrap_or(&identity.package.0);
    format!("{}::{}", package, identity.path.join("::"))
}

pub(crate) fn array_interfaces<'a>(
    providers: impl Clone + Iterator<Item = &'a ModuleDecl>,
    definitions: &DefinitionTable,
) -> Result<BTreeMap<CollectionAccess, DefinitionId>, DeclarationError> {
    let mut types = BTreeMap::new();
    let mut traits = BTreeMap::new();
    for provider in providers.clone() {
        for ty in &provider.types {
            let id = provider.definition(ty.kind.definition_kind(), &ty.name, definitions)?;
            if types.insert(id, ty).is_some() {
                return Err(DeclarationError("duplicate registered type".into()));
            }
        }
        for contract in &provider.traits {
            let id = provider.definition(DefinitionKind::Trait, &contract.name, definitions)?;
            if traits.insert(id, contract).is_some() {
                return Err(DeclarationError("duplicate registered trait".into()));
            }
        }
    }
    let mut interfaces = BTreeMap::new();
    for implementation in providers.flat_map(|module| &module.implementations) {
        let (Ty::NativeObject(nominal), Some(interface)) =
            (&implementation.for_type, &implementation.trait_type)
        else {
            continue;
        };
        definitions
            .resolve(nominal.declaration)
            .map_err(|error| DeclarationError(error.to_string()))?;
        let Some(element) = types
            .get(&nominal.declaration)
            .and_then(|ty| match ty.kind {
                TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element }) => {
                    nominal.arguments.get(element)
                }
                _ => None,
            })
        else {
            continue;
        };
        if interface.arguments.as_slice() != [element.clone()] {
            continue;
        }
        definitions
            .resolve(interface.declaration)
            .map_err(|error| DeclarationError(error.to_string()))?;
        let Some(contract) = traits.get(&interface.declaration) else {
            continue;
        };
        let Some(access) = contract.storage_access else {
            continue;
        };
        if interfaces
            .insert(access, interface.declaration)
            .is_some_and(|previous| previous != interface.declaration)
        {
            return Err(DeclarationError(
                "ambiguous registered array interface".into(),
            ));
        }
    }
    Ok(interfaces)
}
