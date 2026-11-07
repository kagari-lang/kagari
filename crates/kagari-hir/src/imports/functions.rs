//! Imported call contracts are projections of independently checked signatures.

use crate::{
    PreparedAnalysis,
    aggregates::AggregateCatalog,
    declarations::Declaration,
    imports::{SourceDeclRef, SourceItem},
    lower::LoweredModule,
    resolver::resolved::{DeclarationNames, ResolvedName, ResolvedNames},
    semantic::signatures::TypedFunction,
    typeck::table::{CallTarget, TypeTable},
};

use std::collections::HashMap;
use {
    kagari_common::{
        cancellation::{CancellationToken, Cancelled},
        identity::table::DefinitionId,
    },
    kagari_source::identity::FileId,
};

/// A canonical foreign function target and a projection of its independently checked signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFunction {
    /// Qualified source declaration address, independent of aliases.
    pub id: SourceDeclRef,
    /// Portable or scoped semantic definition identity.
    pub declaration: DefinitionId,
    /// Declaration/navigation metadata at the original source site.
    pub site: Declaration,
    /// Callee signature facts; not a second inference of the callee body.
    pub signature: TypedFunction,
}

/// Foreign callable facts keyed by canonical source references or inherent-method definitions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportedFunctions {
    functions: HashMap<SourceDeclRef, ImportedFunction>,
    methods: HashMap<DefinitionId, ImportedFunction>,
}

impl ImportedFunctions {
    pub(crate) fn include_inherent_methods(&mut self, aggregates: &AggregateCatalog) {
        for method in aggregates.inherent_methods() {
            self.methods.insert(
                method.declaration,
                ImportedFunction {
                    id: method.id.clone(),
                    declaration: method.declaration,
                    site: method.site.clone(),
                    signature: method.function.clone(),
                },
            );
        }
    }
}

/// Borrowed prepared modules used to project checked signatures into importer environments.
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

    /// Collects accessible reachable source functions and verifies complete source units before reading signatures.
    pub(crate) fn bindings(
        &self,
        names: &DeclarationNames,
        cancel: &CancellationToken,
    ) -> Result<ImportedFunctions, Cancelled> {
        let mut result = ImportedFunctions::default();
        for source in names.catalog.reachable_sources(&names.items, cancel)? {
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
        if !target
            .unit
            .matches(&module.lowered, &module.names.facts.catalog.definitions)
        {
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
            declaration: *declaration,
            site: module
                .declarations
                .target(ResolvedName::Function(function))
                .expect("function declaration")
                .clone(),
            signature: signature.clone(),
        }))
    }
}

mod mapping;

impl ImportedFunctions {
    /// Finds an inherent/imported callable by semantic definition identity, or `None` if not projected.
    pub fn target(&self, id: &DefinitionId) -> Option<&ImportedFunction> {
        self.methods.get(id).or_else(|| {
            self.functions
                .values()
                .find(|function| function.declaration == *id)
        })
    }

    /// Finds a projected source callable; non-source resolver names return `None`.
    pub fn get(&self, name: ResolvedName) -> Option<&ImportedFunction> {
        match name {
            ResolvedName::Source(source) => self.functions.get(&source),
            _ => None,
        }
    }
}

impl ImportedFunctions {
    /// Compare callable contracts actually read by prior body facts. Namespace
    /// equivalence is a separate required guard: additions, ambiguity, access and
    /// renamed targets cannot be hidden by an absent prior successful call.
    /// Fresh name resolution and arena-aware body restoration install current
    /// source addresses; this predicate never authorizes retaining old navigation.
    pub(crate) fn same_used_contracts(
        &self,
        other: &Self,
        lowered: &LoweredModule,
        names: &ResolvedNames,
        table: &TypeTable,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        let current: HashMap<_, _> = other
            .functions
            .values()
            .chain(other.methods.values())
            .map(|function| (function.declaration, function))
            .collect();
        for (id, _) in lowered.module.body.expressions() {
            cancel.check()?;
            let resolved = names
                .expr_resolution(id)
                .and_then(|target| self.get(target));
            let called = table.call_resolution(id).and_then(|call| {
                if let CallTarget::SourceFunction(id) = call.target {
                    self.target(&id)
                } else {
                    None
                }
            });
            for old in resolved.into_iter().chain(called) {
                let Some(new) = current.get(&old.declaration) else {
                    return Ok(false);
                };
                if old.declaration != new.declaration || old.signature != new.signature {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
