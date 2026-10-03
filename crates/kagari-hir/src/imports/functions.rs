//! Imported call contracts are projections of independently checked signatures.

use crate::{
    PreparedAnalysis,
    aggregates::AggregateCatalog,
    declarations::Declaration,
    hir::{ids::FunctionId, item::storage::ExportItem},
    imports::{ImportTarget, ModuleImports, SourceImport},
    resolver::resolved::ResolvedName,
    typeck::TypedFunction,
};

use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{DefinitionPath, FileId, Revision, reference::DefinitionReference},
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceFunctionId {
    pub file: FileId,
    pub revision: Revision,
    pub function: FunctionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFunction<I: DefinitionReference = DefinitionPath> {
    pub id: SourceFunctionId,
    pub declaration: I,
    pub site: Declaration<I>,
    pub signature: TypedFunction<I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFunctions<I: DefinitionReference = DefinitionPath> {
    functions: HashMap<ResolvedName, ImportedFunction<I>>,
    methods: HashMap<I, ImportedFunction<I>>,
}

impl ImportedFunctions {
    pub(crate) fn include_inherent_methods(&mut self, aggregates: &AggregateCatalog) {
        for method in aggregates.inherent_methods() {
            self.methods.insert(
                method.declaration.clone(),
                ImportedFunction {
                    id: method.id,
                    declaration: method.declaration.clone(),
                    site: method.site.clone(),
                    signature: method.function.clone(),
                },
            );
        }
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
        for (binding, target) in &imports.bindings {
            cancel.check()?;
            let ImportTarget::Source(source) = target else {
                continue;
            };
            if let Some(function) = self.resolve(source, cancel)? {
                result.functions.insert(*binding, function);
            }
        }
        Ok(result)
    }

    fn resolve(
        &self,
        target: &SourceImport,
        cancel: &CancellationToken,
    ) -> Result<Option<ImportedFunction>, Cancelled> {
        cancel.check()?;
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
            site: module
                .declarations
                .target(ResolvedName::Function(function))
                .expect("function declaration")
                .clone(),
            signature: signature.clone(),
        }))
    }
}

impl<I: DefinitionReference> Default for ImportedFunctions<I> {
    fn default() -> Self {
        Self {
            functions: Default::default(),
            methods: Default::default(),
        }
    }
}

mod mapping;

impl<I: DefinitionReference> ImportedFunctions<I> {
    pub fn target(&self, id: &I) -> Option<&ImportedFunction<I>> {
        self.methods.get(id).or_else(|| {
            self.functions
                .values()
                .find(|function| function.declaration == *id)
        })
    }

    pub fn get(&self, name: ResolvedName) -> Option<&ImportedFunction<I>> {
        self.functions.get(&name)
    }
}
