//! Nominal type imports are built from declarations, before signature checking.

use crate::{
    DeclaredAnalysis,
    declarations::{Declaration, DeclarationId},
    imports::{SourceDeclRef, SourceItem, SourceUnit},
    native::NativeTypeKind,
    resolver::resolved::{DeclarationNames, ResolvedName},
    typeck::supertraits::trait_supertrait_surface,
    types::{NominalType, TypeId},
};

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
};
use {
    kagari_common::{
        cancellation::{CancellationToken, Cancelled},
        identity::{DefinitionPath, reference::DefinitionReference},
    },
    kagari_source::identity::FileId,
};

/// A foreign nominal declaration surface prepared before function signature checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedType<I: DefinitionReference = DefinitionPath> {
    /// Installed storage/type descriptor when this is a registered native-backed type.
    pub native_type: Option<NativeTypeKind<I>>,
    /// Associated family name to number of member-level generic parameters.
    pub associated_arities: BTreeMap<String, usize>,
    /// Canonical source declaration address.
    pub id: SourceDeclRef,
    /// Definition and source-site metadata at the original declaration.
    pub declaration: Declaration<I>,
    /// Semantic nominal/type surface with its generic arguments.
    pub ty: TypeId<I>,
    /// Trait method names and semantic identities.
    pub trait_methods: Vec<ImportedTraitMethod<I>>,
    /// Names of declared associated types.
    pub associated_types: Vec<String>,
    /// Prepared parent trait applications.
    pub supertraits: Vec<NominalType<I>>,
}

/// A method-name/definition pair on an imported trait surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTraitMethod<I: DefinitionReference = DefinitionPath> {
    /// Member spelling within the trait.
    pub name: String,
    /// Canonical semantic identity of the method.
    pub declaration: I,
}

/// Canonical type/variant projections shared across importer aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTypes<I: DefinitionReference = DefinitionPath> {
    // Canonical declarations share one surface across local import spellings.
    resolutions: HashMap<SourceDeclRef, ImportedType<I>>,
    nominal_types: HashMap<I, ImportedType<I>>,
    variants: HashMap<SourceDeclRef, Declaration<I>>,
}

/// Borrowed declaration-stage modules and a lazily prepared shared nominal surface cache.
pub(crate) struct TypeCatalog<'a> {
    modules: HashMap<FileId, &'a DeclaredAnalysis>,
    surfaces: RefCell<Option<HashMap<DefinitionPath, ImportedType>>>,
}

impl<'a> TypeCatalog<'a> {
    pub(crate) fn new(modules: impl IntoIterator<Item = &'a DeclaredAnalysis>) -> Self {
        Self {
            surfaces: Default::default(),
            modules: modules
                .into_iter()
                .map(|m| (m.lowered.source.id(), m))
                .collect(),
        }
    }

    /// Projects reachable type/variant declarations and required parent trait surfaces for an importer.
    pub(crate) fn bindings(
        &self,
        names: &DeclarationNames,
        cancel: &CancellationToken,
    ) -> Result<ImportedTypes, Cancelled> {
        self.prepare_surfaces(cancel)?;
        let mut result = ImportedTypes::default();
        for source in names
            .catalog
            .reachable_sources(&names.items, &names.hosts, cancel)?
        {
            cancel.check()?;
            if let Some(ty) = self.resolve(&source, cancel)? {
                result.resolutions.insert(source.clone(), ty);
            }
            if let SourceItem::Variant(variant) = source.item
                && let Some(module) = self.modules.get(&source.unit.file)
                && source.unit.matches(&module.lowered)
                && let Some(declaration) = module.declarations.variant(variant)
            {
                result.variants.insert(source.clone(), declaration.clone());
            }
        }
        let cache = self.surfaces.borrow();
        let mut pending = result
            .resolutions
            .values()
            .filter_map(|item| match &item.ty {
                TypeId::Trait(ty) => Some(ty.declaration.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            cancel.check()?;
            if result.nominal_types.contains_key(&id) {
                continue;
            }
            let Some(item) = cache.as_ref().and_then(|cache| cache.get(&id)) else {
                continue;
            };
            pending.extend(
                item.supertraits
                    .iter()
                    .map(|parent| parent.declaration.clone()),
            );
            result.nominal_types.insert(id, item.clone());
        }
        Ok(result)
    }

    fn resolve(
        &self,
        target: &SourceDeclRef,
        cancel: &CancellationToken,
    ) -> Result<Option<ImportedType>, Cancelled> {
        cancel.check()?;
        let Some(module) = self.modules.get(&target.unit.file) else {
            return Ok(None);
        };
        if !target.unit.matches(&module.lowered) {
            return Ok(None);
        }
        let item = target.item;
        let source = target.clone();
        let Some(surface) = Self::surface(module, source) else {
            return Ok(None);
        };
        let imported = self
            .surfaces
            .borrow()
            .as_ref()
            .and_then(|cache| {
                cache.get(module.declarations.definition(match item {
                    SourceItem::Struct(id) => ResolvedName::Struct(id),
                    SourceItem::Enum(id) => ResolvedName::Enum(id),
                    SourceItem::Trait(id) => ResolvedName::Trait(id),
                    _ => return None,
                })?)
            })
            .cloned();
        Ok(Some(imported.unwrap_or(surface)))
    }

    fn surface(module: &DeclaredAnalysis, source: SourceDeclRef) -> Option<ImportedType> {
        let item = source.item;
        let resolved = match item {
            SourceItem::OpaqueType(id) => ResolvedName::OpaqueType(id),
            SourceItem::Struct(id) => ResolvedName::Struct(id),
            SourceItem::Enum(id) => ResolvedName::Enum(id),
            SourceItem::Trait(id) => ResolvedName::Trait(id),
            _ => return None,
        };
        let declaration = module.declarations.target(resolved.clone())?;
        let identity = module.declarations.definition(resolved)?;
        let native_type = match item {
            SourceItem::OpaqueType(id) => Some(module.declarations.native_type(id)?),
            _ => None,
        };
        let nominal = NominalType {
            associated_types: Default::default(),
            declaration: identity.clone(),
            arguments: module
                .declarations
                .parameters_of(identity)
                .into_iter()
                .map(TypeId::Generic)
                .collect(),
        };
        let ty = match item {
            _ if native_type.is_some() => native_type.as_ref()?.apply(&nominal.arguments)?,
            SourceItem::Struct(_) => TypeId::Struct(nominal),
            SourceItem::Enum(_) => TypeId::Enum(nominal),
            SourceItem::Trait(_) => TypeId::Trait(nominal),
            _ => return None,
        };
        Some(ImportedType {
            native_type,
            associated_arities: match item {
                SourceItem::Trait(id) => module
                    .lowered
                    .module
                    .traits
                    .iter()
                    .find(|item| item.id == id)
                    .into_iter()
                    .flat_map(|item| &item.associated_types)
                    .map(|member| (member.name.clone(), member.generic_params.len()))
                    .collect(),
                _ => Default::default(),
            },
            supertraits: Vec::new(),
            associated_types: match item {
                SourceItem::Trait(id) => module
                    .lowered
                    .module
                    .traits
                    .iter()
                    .find(|item| item.id == id)
                    .into_iter()
                    .flat_map(|item| &item.associated_types)
                    .map(|item| item.name.clone())
                    .collect(),
                _ => Vec::new(),
            },
            id: source,
            declaration: declaration.clone(),
            ty,
            trait_methods: match item {
                SourceItem::Trait(id) => module
                    .lowered
                    .module
                    .traits
                    .iter()
                    .find(|item| item.id == id)
                    .into_iter()
                    .flat_map(|item| &item.methods)
                    .filter_map(|method| {
                        Some(ImportedTraitMethod {
                            name: method.name.clone(),
                            declaration: module
                                .declarations
                                .definition(ResolvedName::Function(method.function))?
                                .clone(),
                        })
                    })
                    .collect(),
                _ => Vec::new(),
            },
        })
    }

    fn prepare_surfaces(&self, cancel: &CancellationToken) -> Result<(), Cancelled> {
        if self.surfaces.borrow().is_some() {
            return Ok(());
        }
        let mut initial = HashMap::new();
        for module in self.modules.values() {
            for item in &module.lowered.module.traits {
                cancel.check()?;
                let source = SourceDeclRef {
                    unit: SourceUnit::of(&module.lowered),
                    item: SourceItem::Trait(item.id),
                };
                if let Some(surface) = Self::surface(module, source) {
                    let TypeId::Trait(ty) = &surface.ty else {
                        unreachable!("trait surface");
                    };
                    initial.insert(ty.declaration.clone(), surface);
                }
            }
        }
        *self.surfaces.borrow_mut() = Some(initial);
        // Parents can use projections justified by another imported parent chain.
        // Refine declaration surfaces before function signature resolution.
        for _ in 0..64 {
            let previous = self.surfaces.borrow().as_ref().unwrap().clone();
            let mut next = previous.clone();
            for module in self.modules.values() {
                cancel.check()?;
                let mut declarations = module.declarations.clone();
                declarations.imported_types = self.bindings(module.names.facts(), cancel)?;
                for item in &module.lowered.module.traits {
                    let Some(id) = declarations.definition(ResolvedName::Trait(item.id)) else {
                        continue;
                    };
                    if let Some(surface) = next.get_mut(id) {
                        surface.supertraits = trait_supertrait_surface(
                            &module.lowered.module,
                            item,
                            &declarations,
                            cancel,
                        );
                    }
                }
            }
            let unchanged = next == previous;
            *self.surfaces.borrow_mut() = Some(next);
            if unchanged {
                break;
            }
        }
        Ok(())
    }
}

impl<I: DefinitionReference> Default for ImportedTypes<I> {
    fn default() -> Self {
        Self {
            resolutions: Default::default(),
            nominal_types: Default::default(),
            variants: Default::default(),
        }
    }
}

mod mapping;

impl<I: DefinitionReference> ImportedTypes<I> {
    /// Iterates canonical imported native representations, independent of aliases.
    pub(crate) fn native_types(&self) -> impl Iterator<Item = &NativeTypeKind<I>> {
        self.resolutions
            .values()
            .filter_map(|ty| ty.native_type.as_ref())
    }

    pub(crate) fn variant(&self, name: ResolvedName) -> Option<&Declaration<I>> {
        match name {
            ResolvedName::Source(source) => self.variants.get(&source),
            _ => None,
        }
    }

    /// Looks up a qualified source type name; other resolver target kinds return `None`.
    pub fn resolved(&self, name: ResolvedName) -> Option<&ImportedType<I>> {
        match name {
            ResolvedName::Source(source) => self.resolutions.get(&source),
            _ => None,
        }
    }

    /// Finds an imported type by its exact source declaration address.
    pub fn target(&self, id: SourceDeclRef) -> Option<&ImportedType<I>> {
        self.resolutions.values().find(|ty| ty.id == id)
    }

    /// Finds a projected nominal surface by semantic definition identity.
    pub fn by_declaration(&self, id: &I) -> Option<&ImportedType<I>> {
        self.resolutions
            .values()
            .chain(self.nominal_types.values())
            .find(|ty| matches!(&ty.declaration.id, DeclarationId::Definition(declaration) if declaration == id))
    }
}
