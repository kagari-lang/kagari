//! Imported call contracts are projections of independently checked signatures.
use super::*;
use crate::{
    PreparedAnalysis, declarations::Declaration, resolver::ResolvedName, typeck::TypedFunction,
};

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
    pub site: Declaration,
    pub signature: TypedFunction,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedFunctions {
    functions: HashMap<ResolvedName, ImportedFunction>,
    methods: HashMap<SourceFunctionId, ImportedFunction>,
}

impl ImportedFunctions {
    pub fn target(&self, id: SourceFunctionId) -> Option<&ImportedFunction> {
        self.methods
            .get(&id)
            .or_else(|| self.functions.values().find(|function| function.id == id))
    }
    pub fn get(&self, name: ResolvedName) -> Option<&ImportedFunction> {
        self.functions.get(&name)
    }
    pub(crate) fn include_inherent_methods(
        &mut self,
        aggregates: &crate::aggregates::AggregateCatalog,
    ) {
        for method in aggregates.inherent_methods() {
            self.methods.insert(
                method.id,
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
