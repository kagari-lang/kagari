//! Immutable language contracts are available independently of optional modules.
use crate::native::{
    binding::NativeResult,
    catalog::DeclarationCatalog,
    types::{TraitRef, Type},
};
use kagari_abi::{
    declaration::ModuleDecl,
    language::{self, Protocol, catalog},
    standard::surface::StandardEnum,
    types::AbiType,
};
use kagari_common::collection::CollectionAccess;
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

    pub fn list(&self) -> TraitRef {
        self.protocol(Protocol::List)
    }

    pub fn mutable_list(&self) -> TraitRef {
        self.protocol(Protocol::MutableList)
    }

    pub fn map(&self) -> TraitRef {
        self.protocol(Protocol::Map)
    }

    pub fn mutable_map(&self) -> TraitRef {
        self.protocol(Protocol::MutableMap)
    }

    pub fn set(&self) -> TraitRef {
        self.protocol(Protocol::Set)
    }

    pub fn mutable_set(&self) -> TraitRef {
        self.protocol(Protocol::MutableSet)
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
        Type(AbiType::Array(Box::new(item.0), CollectionAccess::Mutable))
    }

    pub fn collection_cursor(&self, item: Type) -> Type {
        Type(AbiType::Iter(Box::new(item.0)))
    }

    pub fn hash_map(&self, key: Type, value: Type) -> Type {
        Type(AbiType::Map {
            key: Box::new(key.0),
            value: Box::new(value.0),
            access: CollectionAccess::Mutable,
        })
    }

    pub fn hash_set(&self, item: Type) -> Type {
        Type(AbiType::Set(Box::new(item.0), CollectionAccess::Mutable))
    }

    pub fn option(&self, item: Type) -> Type {
        Type(AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.0],
        })
    }

    pub fn ordering(&self) -> Type {
        Type(AbiType::StandardEnum {
            kind: StandardEnum::Ordering,
            args: vec![],
        })
    }
}
