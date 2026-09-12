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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedTypes {
    types: HashMap<String, ImportedType>,
}

impl ImportedTypes {
    pub fn get(&self, name: &str) -> Option<&ImportedType> {
        self.types.get(name)
    }
    pub fn target(&self, id: SourceTypeId) -> Option<&ImportedType> {
        self.types.values().find(|ty| ty.id == id)
    }
}

pub(crate) struct TypeCatalog<'a> {
    graph: &'a ModuleGraph,
    modules: HashMap<FileId, &'a DeclaredAnalysis>,
}

impl<'a> TypeCatalog<'a> {
    pub(crate) fn new(
        graph: &'a ModuleGraph,
        modules: impl IntoIterator<Item = &'a DeclaredAnalysis>,
    ) -> Self {
        Self {
            graph,
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
        for import in &imports.entries {
            cancel.check()?;
            let Some(ImportTarget::Source(source)) = &import.target else {
                continue;
            };
            let Some(source) = self.graph.resolve_item(source.clone(), cancel)? else {
                continue;
            };
            if let Some(ty) = self.resolve(source.clone(), cancel)? {
                result.types.insert(import.alias.clone(), ty);
            }
            if source.item.is_none() {
                for (name, items) in source.members.iter() {
                    cancel.check()?;
                    let [item] = items.as_slice() else {
                        continue;
                    };
                    let mut target = source.clone();
                    target.item = Some(*item);
                    if let Some(ty) = self.resolve(target, cancel)? {
                        result.types.insert(format!("{}::{name}", import.alias), ty);
                    }
                }
            }
        }
        Ok(result)
    }

    fn resolve(
        &self,
        target: SourceImport,
        cancel: &CancellationToken,
    ) -> Result<Option<ImportedType>, Cancelled> {
        let Some(target) = self.graph.resolve_item(target, cancel)? else {
            return Ok(None);
        };
        let Some(module) = self.modules.get(&target.file) else {
            return Ok(None);
        };
        let Some(item) = target.item else {
            return Ok(None);
        };
        let (resolved, make_type): (_, fn(kagari_common::identity::DefinitionId) -> TypeId) =
            match item {
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
            id: SourceTypeId {
                file: target.file,
                revision: target.revision,
                item,
            },
            declaration: declaration.clone(),
            ty: make_type(identity.clone()),
        }))
    }
}
