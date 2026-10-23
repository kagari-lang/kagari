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
        identity::table::DefinitionId,
    },
    kagari_source::identity::FileId,
};

/// A foreign nominal declaration surface prepared before function signature checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedType {
    /// Installed storage/type descriptor when this is a registered native-backed type.
    pub native_type: Option<NativeTypeKind>,
    /// Associated family name to number of member-level generic parameters.
    pub associated_arities: BTreeMap<String, usize>,
    /// Canonical source declaration address.
    pub id: SourceDeclRef,
    /// Definition and source-site metadata at the original declaration.
    pub declaration: Declaration,
    /// Semantic nominal/type surface with its generic arguments.
    pub ty: TypeId,
    /// Trait method names and semantic identities.
    pub trait_methods: Vec<ImportedTraitMethod>,
    /// Names of declared associated types.
    pub associated_types: Vec<String>,
    /// Prepared parent trait applications.
    pub supertraits: Vec<NominalType>,
}

/// A method-name/definition pair on an imported trait surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTraitMethod {
    /// Member spelling within the trait.
    pub name: String,
    /// Canonical semantic identity of the method.
    pub declaration: DefinitionId,
}

/// Canonical type/variant projections shared across importer aliases.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportedTypes {
    // Canonical declarations share one surface across local import spellings.
    resolutions: HashMap<SourceDeclRef, ImportedType>,
    nominal_types: HashMap<DefinitionId, ImportedType>,
    variants: HashMap<SourceDeclRef, Declaration>,
    /// Canonical templates, including this unit's aliases and dependency aliases.
    pub(crate) aliases: HashMap<DefinitionId, aliases::CanonicalAlias>,
}

/// Borrowed declaration-stage modules and a lazily prepared shared nominal surface cache.
pub(crate) struct TypeCatalog<'a> {
    modules: HashMap<FileId, &'a DeclaredAnalysis>,
    surfaces: RefCell<Option<HashMap<DefinitionId, ImportedType>>>,
    aliases: RefCell<HashMap<DefinitionId, aliases::CanonicalAlias>>,
}

impl<'a> TypeCatalog<'a> {
    pub(crate) fn new(modules: impl IntoIterator<Item = &'a DeclaredAnalysis>) -> Self {
        Self {
            surfaces: Default::default(),
            aliases: Default::default(),
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
        let mut result = ImportedTypes {
            aliases: self.aliases.borrow().clone(),
            ..Default::default()
        };
        for source in names.catalog.reachable_sources(&names.items, cancel)? {
            cancel.check()?;
            if let Some(ty) = self.resolve(&source, cancel)? {
                result.resolutions.insert(source.clone(), ty);
            }
            if let SourceItem::Variant(variant) = source.item
                && let Some(module) = self.modules.get(&source.unit.file)
                && source
                    .unit
                    .matches(&module.lowered, &module.names.facts.catalog.definitions)
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
                TypeId::Trait(ty) => Some(ty.declaration),
                _ => None,
            })
            .collect::<Vec<_>>();
        // Local projections must observe the same refined generation as imports,
        // including during alias preparation when the alias cache is partial.
        if let Some(unit) = &names.items.unit {
            pending.extend(
                cache
                    .as_ref()
                    .into_iter()
                    .flat_map(|cache| cache.iter())
                    .filter(|(_, surface)| &surface.id.unit == unit)
                    .map(|(id, _)| *id),
            );
        }
        while let Some(id) = pending.pop() {
            cancel.check()?;
            if result.nominal_types.contains_key(&id) {
                continue;
            }
            let Some(item) = cache.as_ref().and_then(|cache| cache.get(&id)) else {
                continue;
            };
            pending.extend(item.supertraits.iter().map(|parent| parent.declaration));
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
        if !target
            .unit
            .matches(&module.lowered, &module.names.facts.catalog.definitions)
        {
            return Ok(None);
        }
        let item = target.item;
        let source = target.clone();
        if let SourceItem::TypeAlias(id) = item {
            let declaration = module
                .declarations
                .target(ResolvedName::TypeAlias(id))
                .cloned();
            return Ok(declaration.map(|declaration| {
                let identity = module
                    .declarations
                    .definition(ResolvedName::TypeAlias(id))
                    .expect("alias declaration");
                ImportedType {
                    native_type: None,
                    associated_arities: Default::default(),
                    id: source,
                    declaration,
                    ty: self
                        .aliases
                        .borrow()
                        .get(identity)
                        .map_or(TypeId::Error, |alias| alias.target.clone()),
                    trait_methods: Vec::new(),
                    associated_types: Vec::new(),
                    supertraits: Vec::new(),
                }
            }));
        }
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
            declaration: *identity,
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
                            declaration: *module
                                .declarations
                                .definition(ResolvedName::Function(method.function))?,
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
                    unit: SourceUnit::of(&module.lowered, &module.names.facts.catalog.definitions),
                    item: SourceItem::Trait(item.id),
                };
                if let Some(surface) = Self::surface(module, source) {
                    let TypeId::Trait(ty) = &surface.ty else {
                        unreachable!("trait surface");
                    };
                    initial.insert(ty.declaration, surface);
                }
            }
        }
        *self.surfaces.borrow_mut() = Some(initial);
        self.prepare_aliases(cancel)?;
        // Alias targets can project inherited members, while parent arguments
        // can contain aliases. Refine both against the preceding surface rather
        // than permanently caching failures from an incomplete dependency view.
        for _ in 0..64 {
            let previous_aliases = self.aliases.borrow().clone();
            let previous = self.surfaces.borrow().as_ref().unwrap().clone();
            let mut next = previous.clone();
            for module in self.modules.values() {
                cancel.check()?;
                let mut declarations = module.declarations.clone();
                declarations.imported_types = self.bindings(module.names.facts(), cancel)?;
                declarations.publish_definitions(declarations.context().snapshot());
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
            self.aliases.borrow_mut().clear();
            self.prepare_aliases(cancel)?;
            if unchanged && *self.aliases.borrow() == previous_aliases {
                return Ok(());
            }
        }
        for alias in self.aliases.borrow_mut().values_mut() {
            alias.target = TypeId::Error;
            alias.error = Some(
                "alias and supertrait dependencies did not converge within 64 refinement rounds"
                    .into(),
            );
        }
        Ok(())
    }
}

pub mod aliases;
mod mapping;

impl ImportedTypes {
    /// Canonical transparent alias metadata, keyed by its source declaration identity.
    /// The identity owns binders/navigation, not a runtime type.
    pub fn alias(&self, id: &DefinitionId) -> Option<&aliases::CanonicalAlias> {
        self.aliases.get(id)
    }

    /// Iterates canonical imported native representations, independent of aliases.
    pub(crate) fn native_types(&self) -> impl Iterator<Item = &NativeTypeKind> {
        self.resolutions
            .values()
            .filter_map(|ty| ty.native_type.as_ref())
    }

    pub(crate) fn variant(&self, name: ResolvedName) -> Option<&Declaration> {
        match name {
            ResolvedName::Source(source) => self.variants.get(&source),
            _ => None,
        }
    }

    /// Looks up a qualified source type name; other resolver target kinds return `None`.
    pub fn resolved(&self, name: ResolvedName) -> Option<&ImportedType> {
        match name {
            ResolvedName::Source(source) => self.resolutions.get(&source),
            _ => None,
        }
    }

    /// Finds an imported type by its exact source declaration address.
    pub fn target(&self, id: SourceDeclRef) -> Option<&ImportedType> {
        self.resolutions.values().find(|ty| ty.id == id)
    }

    /// Finds a projected nominal surface by semantic definition identity.
    pub fn by_declaration(&self, id: &DefinitionId) -> Option<&ImportedType> {
        self.resolutions
            .values()
            .chain(self.nominal_types.values())
            .find(|ty| matches!(&ty.declaration.id, DeclarationId::Definition(declaration) if declaration == id))
    }
}
