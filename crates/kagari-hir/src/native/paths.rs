//! Source package spelling and inherent ownership from the installed declarations.
use kagari_common::identity::{DefinitionKind, DefinitionPath, ModuleIdentity};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeStorageLayout,
    },
    ty::Ty,
};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn module_path(identity: &ModuleIdentity, providers: &[Arc<ModuleDecl>]) -> String {
    let package = providers
        .iter()
        .find(|module| module.identity == *identity)
        .and_then(|module| module.package_alias.as_deref())
        .unwrap_or(&identity.package.0);
    format!("{}::{}", package, identity.path.join("::"))
}

pub(crate) fn array_interfaces(
    providers: &[Arc<ModuleDecl>],
) -> Result<BTreeMap<CollectionAccess, DefinitionPath>, DeclarationError> {
    let mut interfaces = BTreeMap::new();
    for implementation in providers.iter().flat_map(|module| &module.implementations) {
        let (Ty::NativeObject(nominal), Some(interface)) =
            (&implementation.for_type, &implementation.trait_type)
        else {
            continue;
        };
        let Some(element) = providers
            .iter()
            .find(|module| module.identity == nominal.declaration.module)
            .and_then(|module| {
                module.types.iter().find(|ty| {
                    module.definition(ty.kind.definition_kind(), &ty.name) == nominal.declaration
                })
            })
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
        let Some(contract) = providers
            .iter()
            .find(|module| module.identity == interface.declaration.module)
            .and_then(|module| {
                module.traits.iter().find(|contract| {
                    module.definition(DefinitionKind::Trait, &contract.name)
                        == interface.declaration
                })
            })
        else {
            continue;
        };
        let Some(access) = contract.storage_access else {
            continue;
        };
        if interfaces
            .insert(access, interface.declaration.clone())
            .is_some_and(|previous| previous != interface.declaration)
        {
            return Err(DeclarationError(
                "ambiguous registered array interface".into(),
            ));
        }
    }
    Ok(interfaces)
}
