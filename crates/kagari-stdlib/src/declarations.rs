//! Immutable language contracts are available independently of optional modules.
use crate::{catalog::semantic::Semantic, identity};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::native::{
    binding::NativeResult,
    catalog::DeclarationCatalog,
    types::{TraitRef, Type, TypeRef},
};
use kagari_types::{declaration::module::ModuleDecl, language, language::Protocol};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct StandardDeclarations {
    semantic: Arc<Semantic>,
}

impl StandardDeclarations {
    pub(crate) fn new(semantic: Arc<Semantic>) -> Self {
        Self { semantic }
    }

    /// Native owner and facade inputs. Authored owners remain ordinary source modules.
    pub fn declarations(&self) -> &[Arc<DefinitionMetadata<ModuleDecl>>] {
        self.semantic.shared()
    }

    /// Scoped mixed authoring contracts; this view installs no implementation.
    pub fn catalog(&self) -> &DeclarationCatalog {
        self.semantic.catalog()
    }

    pub fn protocol(&self, protocol: Protocol) -> TraitRef {
        self.catalog()
            .trait_reference(&language::identity(protocol))
            .expect("declared standard protocol")
    }

    fn library_trait(&self, name: &str) -> TraitRef {
        self.catalog()
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

    /// Retrieve a library enum's ordinary authoring handle.
    pub fn enumeration(&self, name: &str) -> NativeResult<TypeRef> {
        self.semantic.enumeration_named(name).cloned()
    }

    pub fn option(&self, item: Type) -> NativeResult<Type> {
        self.enumeration("Option")?.apply([item])
    }

    pub fn ordering(&self) -> NativeResult<Type> {
        self.enumeration("Ordering")?.apply([])
    }
}
