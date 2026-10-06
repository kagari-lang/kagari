//! Imported call contracts are projections of independently checked signatures.

use crate::{
    PreparedAnalysis,
    aggregates::AggregateCatalog,
    declarations::Declaration,
    imports::{SourceDeclRef, SourceItem},
    resolver::resolved::{DeclarationNames, ResolvedName},
    typeck::TypedFunction,
};

use std::collections::HashMap;
use {
    kagari_common::{
        cancellation::{CancellationToken, Cancelled},
        identity::{DefinitionPath, reference::DefinitionReference},
    },
    kagari_source::identity::FileId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFunction<I: DefinitionReference = DefinitionPath> {
    pub id: SourceDeclRef,
    pub declaration: I,
    pub site: Declaration<I>,
    pub signature: TypedFunction<I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFunctions<I: DefinitionReference = DefinitionPath> {
    functions: HashMap<SourceDeclRef, ImportedFunction<I>>,
    methods: HashMap<I, ImportedFunction<I>>,
}

impl ImportedFunctions {
    pub(crate) fn include_inherent_methods(&mut self, aggregates: &AggregateCatalog) {
        for method in aggregates.inherent_methods() {
            self.methods.insert(
                method.declaration.clone(),
                ImportedFunction {
                    id: method.id.clone(),
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
        names: &DeclarationNames,
        cancel: &CancellationToken,
    ) -> Result<ImportedFunctions, Cancelled> {
        let mut result = ImportedFunctions::default();
        for source in names
            .catalog
            .reachable_sources(&names.items, &names.hosts, cancel)?
        {
            cancel.check()?;
            if let Some(function) = self.resolve(&source, cancel)? {
                result.functions.insert(source.clone(), function);
            }
        }
        Ok(result)
    }

    fn resolve(
        &self,
        target: &SourceDeclRef,
        cancel: &CancellationToken,
    ) -> Result<Option<ImportedFunction>, Cancelled> {
        cancel.check()?;
        let Some(module) = self.modules.get(&target.unit.file) else {
            return Ok(None);
        };
        let SourceItem::Function(function) = target.item else {
            return Ok(None);
        };
        if !target.unit.matches(&module.lowered) {
            return Ok(None);
        }
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
            id: target.clone(),
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
        match name {
            ResolvedName::Source(source) => self.functions.get(&source),
            _ => None,
        }
    }
}
