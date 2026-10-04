//! Source package spelling and inherent ownership from the installed declarations.
use kagari_common::identity::{DefinitionKind, DefinitionPath, ModuleIdentity};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeTypeConstructor,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(crate) fn module_path(identity: &ModuleIdentity, providers: &[Arc<ModuleDecl>]) -> String {
    let package = providers
        .iter()
        .find(|module| module.identity == *identity)
        .and_then(|module| module.package_alias.as_deref())
        .unwrap_or(&identity.package.0);
    format!("{}::{}", package, identity.path.join("::"))
}

pub(crate) fn receiver_owner(
    receiver: &Ty,
    providers: &[Arc<ModuleDecl>],
) -> Option<ModuleIdentity> {
    match receiver {
        Ty::Struct(nominal) | Ty::Enum(nominal) | Ty::NativeObject(nominal) => {
            return Some(nominal.declaration.module.clone());
        }
        Ty::Builtin(kind) if *kind != BuiltinType::String => {
            return providers
                .iter()
                .find(|module| {
                    module.implementations.iter().any(|implementation| {
                        implementation.trait_type.is_none() && implementation.for_type == *receiver
                    })
                })
                .map(|module| module.identity.clone());
        }
        _ => {}
    }
    let constructor = match receiver {
        Ty::Builtin(BuiltinType::String) => NativeTypeConstructor::String,
        Ty::Array(..) => NativeTypeConstructor::Array,
        Ty::Map { .. } => NativeTypeConstructor::Map,
        Ty::Set(..) => NativeTypeConstructor::Set,
        Ty::Iter(_) => NativeTypeConstructor::Iter,
        Ty::Range(_, kind) => NativeTypeConstructor::Range(*kind),
        Ty::StandardEnum { kind, .. } => NativeTypeConstructor::Enum(*kind),
        _ => return None,
    };
    let owners: BTreeSet<_> = providers
        .iter()
        .filter(|module| {
            module
                .types
                .iter()
                .any(|ty| ty.kind == TypeDefKind::Native(constructor))
        })
        .map(|module| module.identity.clone())
        .collect();
    (owners.len() == 1)
        .then(|| owners.into_iter().next())
        .flatten()
}

pub(crate) fn array_interfaces(
    providers: &[Arc<ModuleDecl>],
) -> Result<BTreeMap<CollectionAccess, DefinitionPath>, DeclarationError> {
    let mut interfaces = BTreeMap::new();
    for implementation in providers.iter().flat_map(|module| &module.implementations) {
        let (Ty::Array(element, _), Some(interface)) =
            (&implementation.for_type, &implementation.trait_type)
        else {
            continue;
        };
        if interface.arguments.as_slice() != [element.as_ref().clone()] {
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
