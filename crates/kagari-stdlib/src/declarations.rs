//! Immutable language contracts are available independently of optional modules.
use crate::{catalog, identity};
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        types::{TraitRef, Type, TypeRef},
    },
};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        TypeDefKind,
        module::{DeclarationError, ModuleDecl},
    },
    language,
    language::Protocol,
};
use std::{cell::OnceCell, collections::BTreeMap, sync::Arc};

thread_local! {
    static ENUM_TYPES: OnceCell<NativeResult<BTreeMap<String, TypeRef>>> = const { OnceCell::new() };
}

#[derive(Debug, Clone)]
pub struct StandardDeclarations {
    declarations: Result<Vec<Arc<DefinitionMetadata<ModuleDecl>>>, DeclarationError>,
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
    /// Borrow the retained scoped inputs, reporting any foundation construction error.
    pub fn declarations(&self) -> NativeResult<&[Arc<DefinitionMetadata<ModuleDecl>>]> {
        self.declarations
            .as_deref()
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))
    }

    pub fn catalog(&self) -> NativeResult<DeclarationCatalog> {
        self.providers
            .get_or_init(|| {
                DeclarationCatalog::from_declarations(self.declarations()?.iter().map(Arc::as_ref))
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

    pub fn vec(&self, item: Type) -> NativeResult<Type> {
        self.catalog()?
            .type_reference(&identity::type_id("Vec", DefinitionKind::AssociatedType))?
            .apply([item])
    }

    pub fn collection_cursor(&self, item: Type) -> NativeResult<Type> {
        Type::iterator(item)
    }

    pub fn hash_map(&self, key: Type, value: Type) -> NativeResult<Type> {
        Type::map(key, value, CollectionAccess::Mutable)
    }

    pub fn hash_set(&self, item: Type) -> NativeResult<Type> {
        Type::set(item, CollectionAccess::Mutable)
    }

    /// Retrieve a library enum's ordinary authoring handle.
    pub fn enumeration(name: &str) -> NativeResult<TypeRef> {
        ENUM_TYPES.with(|cache| {
            let declarations = cache
                .get_or_init(|| {
                    let standard = Self::default();
                    let catalog = standard.catalog()?;
                    standard
                        .declarations()?
                        .iter()
                        .flat_map(|module| {
                            module
                                .records()
                                .types
                                .iter()
                                .filter(|ty| ty.kind == TypeDefKind::Enum)
                                .map(move |ty| (module, ty))
                        })
                        .map(|(_, ty)| {
                            Ok((
                                ty.name.clone(),
                                catalog.type_reference(&identity::type_id(
                                    &ty.name,
                                    ty.kind.definition_kind(),
                                ))?,
                            ))
                        })
                        .collect::<NativeResult<_>>()
                })
                .as_ref()
                .map_err(Clone::clone)?;
            declarations
                .get(name)
                .cloned()
                .ok_or_else(|| RuntimeError::metadata_conflict("unknown standard enum"))
        })
    }

    pub fn option(&self, item: Type) -> NativeResult<Type> {
        Self::enumeration("Option")?.apply([item])
    }

    pub fn ordering(&self) -> NativeResult<Type> {
        Self::enumeration("Ordering")?.apply([])
    }
}
