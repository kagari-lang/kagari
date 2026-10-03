//! Immutable language contracts are available independently of optional modules.
use crate::native::{
    binding::NativeResult,
    catalog::DeclarationCatalog,
    types::{TraitRef, Type},
};
use kagari_common::collection::CollectionAccess;
use kagari_contract::library::{self, catalog};
use kagari_contract::{
    declaration::ModuleDecl,
    language::{self, Protocol},
    standard::surface::StandardEnum,
    types::Ty,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct LanguageContracts {
    declarations: Arc<ModuleDecl>,
}

impl Default for LanguageContracts {
    fn default() -> Self {
        Self {
            declarations: catalog::shared(),
        }
    }
}

impl LanguageContracts {
    pub fn declarations(&self) -> &Arc<ModuleDecl> {
        &self.declarations
    }

    pub(crate) fn catalog(&self) -> NativeResult<DeclarationCatalog> {
        DeclarationCatalog::declared([self.declarations.as_ref()])
    }

    pub fn protocol(&self, protocol: Protocol) -> TraitRef {
        TraitRef {
            id: language::identity(protocol),
            contract: Arc::new(
                self.declarations
                    .traits
                    .iter()
                    .find(|contract| contract.name == protocol.name())
                    .expect("language contract")
                    .clone(),
            ),
        }
    }

    fn library_trait(&self, name: &str) -> TraitRef {
        TraitRef {
            id: library::trait_id(name),
            contract: Arc::new(
                self.declarations
                    .traits
                    .iter()
                    .find(|contract| contract.name == name)
                    .expect("installed library trait")
                    .clone(),
            ),
        }
    }

    pub fn list(&self) -> TraitRef {
        self.library_trait("List")
    }

    pub fn mutable_list(&self) -> TraitRef {
        self.library_trait("MutableList")
    }

    pub fn map(&self) -> TraitRef {
        self.library_trait("Map")
    }

    pub fn mutable_map(&self) -> TraitRef {
        self.library_trait("MutableMap")
    }

    pub fn set(&self) -> TraitRef {
        self.library_trait("Set")
    }

    pub fn mutable_set(&self) -> TraitRef {
        self.library_trait("MutableSet")
    }

    pub fn eq(&self) -> TraitRef {
        self.protocol(Protocol::Eq)
    }

    pub fn hash(&self) -> TraitRef {
        self.protocol(Protocol::Hash)
    }

    pub fn ord(&self) -> TraitRef {
        self.protocol(Protocol::Ord)
    }

    pub fn index(&self) -> TraitRef {
        self.protocol(Protocol::Index)
    }

    pub fn iterator(&self) -> TraitRef {
        self.protocol(Protocol::Iterator)
    }

    pub fn iterable(&self) -> TraitRef {
        self.protocol(Protocol::Iterable)
    }

    pub fn array_list(&self, item: Type) -> Type {
        Type(Ty::Array(Box::new(item.0), CollectionAccess::Mutable))
    }

    pub fn collection_cursor(&self, item: Type) -> Type {
        Type(Ty::Iter(Box::new(item.0)))
    }

    pub fn hash_map(&self, key: Type, value: Type) -> Type {
        Type(Ty::Map {
            key: Box::new(key.0),
            value: Box::new(value.0),
            access: CollectionAccess::Mutable,
        })
    }

    pub fn hash_set(&self, item: Type) -> Type {
        Type(Ty::Set(Box::new(item.0), CollectionAccess::Mutable))
    }

    pub fn option(&self, item: Type) -> Type {
        Type(Ty::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.0],
        })
    }

    pub fn ordering(&self) -> Type {
        Type(Ty::StandardEnum {
            kind: StandardEnum::Ordering,
            args: vec![],
        })
    }
}
