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
        self.types.values().find(|ty| match &ty.ty {
            TypeId::Struct(ty) | TypeId::Enum(ty) | TypeId::Trait(ty) => &ty.declaration == id,
            _ => false,
        })
    }
}

pub(crate) struct TypeCatalog<'a> {
    modules: HashMap<FileId, &'a DeclaredAnalysis>,
}

impl<'a> TypeCatalog<'a> {
    pub(crate) fn new(modules: impl IntoIterator<Item = &'a DeclaredAnalysis>) -> Self {
        Self {
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
        let (resolved, make_type): (_, fn(crate::types::NominalType) -> TypeId) = match item {
            ExportItem::Struct(id) => (ResolvedName::Struct(id), TypeId::Struct),
            ExportItem::Enum(id) => (ResolvedName::Enum(id), TypeId::Enum),
            ExportItem::Trait(id) => (ResolvedName::Trait(id), TypeId::Trait),
            _ => return Ok(None),
        };
        let Some(declaration) = module.declarations.target(resolved) else {
            return Ok(None);
        };
        let Some(identity) = module.declarations.definition(resolved) else {
            return Ok(None);
        };
        Ok(Some(ImportedType {
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
                file: target.file,
                revision: target.revision,
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
        }))
    }
}
