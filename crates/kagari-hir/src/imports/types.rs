//! Nominal type imports are built from declarations, before signature checking.
use super::*;
use crate::{DeclaredAnalysis, declarations::Declaration, resolver::ResolvedName, types::TypeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceTypeId {
    pub file: FileId,
    pub revision: Revision,
    pub item: ExportItem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedType {
    pub id: SourceTypeId,
    pub declaration: Declaration,
    pub ty: TypeId,
    pub trait_methods: Vec<ImportedTraitMethod>,
    pub associated_types: Vec<String>,
    pub supertraits: Vec<crate::types::NominalType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTraitMethod {
    pub name: String,
    pub declaration: kagari_common::identity::DefinitionId,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedTypes {
    types: HashMap<String, ImportedType>,
    resolutions: HashMap<ResolvedName, String>,
    nominal_types: HashMap<kagari_common::identity::DefinitionId, ImportedType>,
}

impl ImportedTypes {
    pub fn resolved(&self, name: ResolvedName) -> Option<&ImportedType> {
        self.types.get(self.resolutions.get(&name)?)
    }

    pub fn get(&self, name: &str) -> Option<&ImportedType> {
        self.types.get(name)
    }
    pub fn target(&self, id: SourceTypeId) -> Option<&ImportedType> {
        self.types.values().find(|ty| ty.id == id)
    }

    pub fn by_declaration(
        &self,
        id: &kagari_common::identity::DefinitionId,
    ) -> Option<&ImportedType> {
        self.types
            .values()
            .chain(self.nominal_types.values())
            .find(|ty| match &ty.ty {
                TypeId::Struct(ty) | TypeId::Enum(ty) | TypeId::Trait(ty) => &ty.declaration == id,
                _ => false,
            })
    }
}

pub(crate) struct TypeCatalog<'a> {
    modules: HashMap<FileId, &'a DeclaredAnalysis>,
    surfaces:
        std::cell::RefCell<Option<HashMap<kagari_common::identity::DefinitionId, ImportedType>>>,
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

    pub(crate) fn bindings(
        &self,
        imports: &ModuleImports,
        cancel: &CancellationToken,
    ) -> Result<ImportedTypes, Cancelled> {
        self.prepare_surfaces(cancel)?;
        let mut result = ImportedTypes::default();
        for (index, import) in imports.entries.iter().enumerate() {
            cancel.check()?;
            let Some(ImportTarget::Source(source)) =
                imports.binding(ResolvedName::SourceImport(index))
            else {
                continue;
            };
            if let Some(ty) = self.resolve(source, cancel)? {
                result.types.insert(import.alias.clone(), ty);
                result
                    .resolutions
                    .insert(ResolvedName::SourceImport(index), import.alias.clone());
            }
            if source.item.is_none() {
                for (name, items) in source.members.iter() {
                    cancel.check()?;
                    let [item] = items.as_slice() else {
                        continue;
                    };
                    let key = ResolvedName::SourceItem {
                        import: index,
                        item: *item,
                    };
                    let Some(ImportTarget::Source(target)) = imports.binding(key) else {
                        continue;
                    };
                    if let Some(ty) = self.resolve(target, cancel)? {
                        let name = format!("{}::{name}", import.alias);
                        result.resolutions.insert(
                            ResolvedName::SourceItem {
                                import: index,
                                item: *item,
                            },
                            name.clone(),
                        );
                        result.types.insert(name, ty);
                    }
                }
            }
        }
        let cache = self.surfaces.borrow();
        let mut pending = result
            .types
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
        target: &SourceImport,
        cancel: &CancellationToken,
    ) -> Result<Option<ImportedType>, Cancelled> {
        cancel.check()?;
        let Some(module) = self.modules.get(&target.file) else {
            return Ok(None);
        };
        let Some(item) = target.item else {
            return Ok(None);
        };
        let source = SourceTypeId {
            file: target.file,
            revision: target.revision,
            item,
        };
        let Some(surface) = Self::surface(module, source) else {
            return Ok(None);
        };
        let imported = self
            .surfaces
            .borrow()
            .as_ref()
            .and_then(|cache| {
                cache.get(module.declarations.definition(match item {
                    ExportItem::Struct(id) => ResolvedName::Struct(id),
                    ExportItem::Enum(id) => ResolvedName::Enum(id),
                    ExportItem::Trait(id) => ResolvedName::Trait(id),
                    _ => return None,
                })?)
            })
            .cloned();
        Ok(Some(imported.unwrap_or(surface)))
    }

    fn surface(module: &DeclaredAnalysis, source: SourceTypeId) -> Option<ImportedType> {
        let item = source.item;
        let (resolved, make_type): (_, fn(crate::types::NominalType) -> TypeId) = match item {
            ExportItem::Struct(id) => (ResolvedName::Struct(id), TypeId::Struct),
            ExportItem::Enum(id) => (ResolvedName::Enum(id), TypeId::Enum),
            ExportItem::Trait(id) => (ResolvedName::Trait(id), TypeId::Trait),
            _ => return None,
        };
        let declaration = module.declarations.target(resolved)?;
        let identity = module.declarations.definition(resolved)?;
        Some(ImportedType {
            supertraits: Vec::new(),
            associated_types: match item {
                ExportItem::Trait(id) => module
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
            id: SourceTypeId {
                file: source.file,
                revision: source.revision,
                item,
            },
            declaration: declaration.clone(),
            ty: make_type(crate::types::NominalType {
                associated_types: Default::default(),
                declaration: identity.clone(),
                arguments: module
                    .declarations
                    .parameters_of(identity)
                    .into_iter()
                    .map(TypeId::Generic)
                    .collect(),
            }),
            trait_methods: match item {
                ExportItem::Trait(id) => module
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
                let source = SourceTypeId {
                    file: module.lowered.source.id(),
                    revision: module.lowered.source.revision(),
                    item: ExportItem::Trait(item.id),
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
                declarations.imported_types =
                    self.bindings(&module.names.facts().imports, cancel)?;
                for item in &module.lowered.module.traits {
                    let Some(id) = declarations.definition(ResolvedName::Trait(item.id)) else {
                        continue;
                    };
                    if let Some(surface) = next.get_mut(id) {
                        surface.supertraits = crate::typeck::trait_supertrait_surface(
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
