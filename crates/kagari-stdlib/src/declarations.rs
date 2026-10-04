//! Immutable language contracts are available independently of optional modules.
use crate::{catalog, identity};
use kagari_runtime::native::{
    binding::NativeResult,
    catalog::DeclarationCatalog,
    types::{TraitRef, Type},
};
use kagari_types::{
    collection::CollectionAccess, declaration::module::ModuleDecl, language, language::Protocol,
    surface::StandardEnum, ty::Ty,
};
use std::{cell::OnceCell, sync::Arc};

#[derive(Debug, Clone)]
pub struct StandardDeclarations {
    declarations: Vec<Arc<ModuleDecl>>,
    providers: OnceCell<NativeResult<DeclarationCatalog>>,
}

impl Default for StandardDeclarations {
    fn default() -> Self {
        Self {
            declarations: catalog::shared(),
            providers: OnceCell::new(),
        }
    }
}

impl StandardDeclarations {
    pub fn declarations(&self) -> &[Arc<ModuleDecl>] {
        &self.declarations
    }

    pub fn catalog(&self) -> NativeResult<DeclarationCatalog> {
        self.providers
            .get_or_init(|| {
                DeclarationCatalog::from_declarations(self.declarations.iter().map(Arc::as_ref))
            })
            .clone()
    }

    pub fn protocol(&self, protocol: Protocol) -> TraitRef {
        self.catalog()
            .expect("checked standard providers")
            .trait_reference(&language::identity(protocol))
            .expect("declared standard protocol")
    }

    fn library_trait(&self, name: &str) -> TraitRef {
        self.catalog()
            .expect("checked standard providers")
            .trait_reference(&identity::trait_id(name))
            .expect("declared standard library trait")
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

    pub fn vec(&self, item: Type) -> Type {
        Type::from_semantic(Ty::Array(
            Box::new(item.abi().clone()),
            CollectionAccess::Mutable,
        ))
    }

    pub fn collection_cursor(&self, item: Type) -> Type {
        Type::from_semantic(Ty::Iter(Box::new(item.abi().clone())))
    }

    pub fn hash_map(&self, key: Type, value: Type) -> Type {
        Type::from_semantic(Ty::Map {
            key: Box::new(key.abi().clone()),
            value: Box::new(value.abi().clone()),
            access: CollectionAccess::Mutable,
        })
    }

    pub fn hash_set(&self, item: Type) -> Type {
        Type::from_semantic(Ty::Set(
            Box::new(item.abi().clone()),
            CollectionAccess::Mutable,
        ))
    }

    pub fn option(&self, item: Type) -> Type {
        Type::from_semantic(Ty::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.abi().clone()],
        })
    }

    pub fn ordering(&self) -> Type {
        Type::from_semantic(Ty::StandardEnum {
            kind: StandardEnum::Ordering,
            args: vec![],
        })
    }
}
