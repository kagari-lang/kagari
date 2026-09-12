//! Imported call contracts are projections of independently checked signatures.
use super::*;
use crate::{PreparedAnalysis, resolver::ResolvedName, typeck::TypedFunction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceFunctionId {
    pub file: FileId,
    pub revision: Revision,
    pub function: crate::hir::FunctionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFunction {
    pub id: SourceFunctionId,
    pub declaration: kagari_common::identity::DefinitionId,
    pub signature: TypedFunction,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedFunctions {
    functions: HashMap<ResolvedName, ImportedFunction>,
}

impl ImportedFunctions {
    pub fn target(&self, id: SourceFunctionId) -> Option<&ImportedFunction> {
        self.functions.values().find(|function| function.id == id)
    }
    pub fn get(&self, name: ResolvedName) -> Option<&ImportedFunction> {
        self.functions.get(&name)
    }
}

pub(crate) struct FunctionCatalog<'a> {
    graph: &'a ModuleGraph,
    modules: HashMap<FileId, &'a PreparedAnalysis>,
}

impl<'a> FunctionCatalog<'a> {
    pub(crate) fn new(
        graph: &'a ModuleGraph,
        modules: impl IntoIterator<Item = &'a PreparedAnalysis>,
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
    ) -> Result<ImportedFunctions, Cancelled> {
        let mut result = ImportedFunctions::default();
        for (index, import) in imports.entries.iter().enumerate() {
            cancel.check()?;
            let Some(ImportTarget::Source(source)) = &import.target else {
                continue;
            };
            if let Some(function) = self.resolve(source.clone(), cancel)? {
                result
                    .functions
                    .insert(ResolvedName::SourceImport(index), function);
            }
            if source.item.is_none() {
                for items in source.members.values() {
                    cancel.check()?;
                    let [item] = items.as_slice() else {
                        continue;
                    };
                    let mut target = source.clone();
                    target.item = Some(*item);
                    if let Some(function) = self.resolve(target, cancel)? {
                        result.functions.insert(
                            ResolvedName::SourceItem {
                                import: index,
                                item: *item,
                            },
                            function,
                        );
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
    ) -> Result<Option<ImportedFunction>, Cancelled> {
        let Some(target) = self.graph.resolve_item(target, cancel)? else {
            return Ok(None);
        };
        let Some(module) = self.modules.get(&target.file) else {
            return Ok(None);
        };
        let Some(ExportItem::Function(function)) = target.item else {
            return Ok(None);
        };
        let Some(signature) = module
            .signatures
            .facts
            .functions
            .iter()
            .find(|f| f.id == function)
        else {
            return Ok(None);
        };
        let Some(declaration) = module
            .declarations
            .definition(ResolvedName::Function(function))
        else {
            return Ok(None);
        };
        Ok(Some(ImportedFunction {
            id: SourceFunctionId {
                file: target.file,
                revision: target.revision,
                function,
            },
            declaration: declaration.clone(),
            signature: signature.clone(),
        }))
    }
}
