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
    pub fn get(&self, name: ResolvedName) -> Option<&ImportedFunction> {
        self.functions.get(&name)
    }
}

pub(crate) struct FunctionCatalog<'a> {
    modules: HashMap<FileId, &'a PreparedAnalysis>,
}

impl<'a> FunctionCatalog<'a> {
    pub(crate) fn new(modules: impl IntoIterator<Item = &'a PreparedAnalysis>) -> Self {
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
        mut target: SourceImport,
        cancel: &CancellationToken,
    ) -> Result<Option<ImportedFunction>, Cancelled> {
        let mut visited = HashSet::new();
        loop {
            cancel.check()?;
            let Some(module) = self.modules.get(&target.file) else {
                return Ok(None);
            };
            if module.lowered.source.revision() != target.revision
                || module.lowered.source.module_identity() != &target.module
            {
                return Ok(None);
            }
            match target.item {
                Some(ExportItem::Function(function)) => {
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
                    return Ok(Some(ImportedFunction {
                        id: SourceFunctionId {
                            file: target.file,
                            revision: target.revision,
                            function,
                        },
                        declaration: declaration.clone(),
                        signature: signature.clone(),
                    }));
                }
                Some(ExportItem::Import(index)) => {
                    if !visited.insert((target.file, index)) {
                        return Ok(None);
                    }
                    let Some(ImportTarget::Source(next)) = module
                        .names
                        .facts
                        .imports
                        .entries
                        .get(index)
                        .and_then(|i| i.target.as_ref())
                    else {
                        return Ok(None);
                    };
                    target = next.clone();
                }
                _ => return Ok(None),
            }
        }
    }
}
