//! Publish only source spellings verified against this request's solved import graph.
use super::{DeclarationSource, render_module};
use crate::{
    analysis::{
        declarations::{DeclarationSnapshot, FileDeclarations},
        error::AnalysisError,
    },
    hir::item::module::ImportLeaf,
    imports::{
        DirectiveResolution, ImportDirective, ResolvedTarget,
        catalog::{LookupContext, LookupResult},
        environment::ImportEnvironment,
    },
    native::paths::{ImporterPaths, legal_name},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, metadata::DefinitionMetadata, table::DefinitionView},
};
use kagari_types::declaration::{
    module::{DeclarationError, ModuleDecl},
    names::{ExportName, NameNamespace},
};
use std::{collections::BTreeMap, ptr};

impl DeclarationSnapshot {
    /// Effective package bindings, dependencies and preludes for all retained importers.
    pub fn import_environment(&self) -> &ImportEnvironment {
        &self.module_graph().catalog.environment
    }

    /// Renders a retained registration for publication. The generated document must be
    /// bound to the registration's identity with [`Self::import_environment`] when
    /// analyzed as source. Pass an entry from [`Self::native_modules`].
    /// Returns an error if the registration is absent or ordinary `pub use` cannot
    /// express exactly its registered bindings in both namespaces. Cancellation
    /// returns [`AnalysisError::Cancelled`] without returning a partial document.
    pub fn render_native_document(
        &self,
        metadata: &DefinitionMetadata<ModuleDecl>,
        cancel: &CancellationToken,
    ) -> Result<DeclarationSource, AnalysisError> {
        cancel.check()?;
        if !self
            .native_modules()
            .iter()
            .any(|m| ptr::eq(m.as_ref(), metadata))
        {
            return Err(DeclarationError(format!(
                "registration for {} does not belong to this declaration snapshot",
                metadata.records().identity
            ))
            .into());
        }
        let importer = &metadata.records().identity;
        let scope = self
            .import_environment()
            .scope(importer)
            .ok_or_else(|| DeclarationError(format!("missing import scope for {importer}")))?;
        let exports = self.publication_exports(metadata, cancel)?;
        let rendered = render_module(
            metadata,
            ImporterPaths::effective(importer, scope),
            &exports,
            cancel,
        );
        // The registration validator and renderer use DeclarationError. Keep
        // cancellation distinct from invalid input at the public analysis boundary.
        cancel.check()?;
        Ok(rendered?)
    }

    fn registration_file(
        &self,
        owner: &ModuleIdentity,
    ) -> Result<&FileDeclarations, DeclarationError> {
        let graph = self.module_graph();
        graph
            .catalog
            .definitions
            .lookup_module(owner)
            .and_then(|id| graph.node(&id))
            .and_then(|node| self.file(node.file))
            .map(AsRef::as_ref)
            .ok_or_else(|| DeclarationError(format!("missing registration owner {owner}")))
    }

    fn matches_definition(&self, target: &ResolvedTarget, expected: DefinitionView<'_>) -> bool {
        self.target_view(target).is_some_and(|actual| {
            actual.module() == expected.module() && actual.segments().eq(expected.segments())
        })
    }

    fn target_view(&self, target: &ResolvedTarget) -> Option<DefinitionView<'_>> {
        let ResolvedTarget::Source(source) = target else {
            return None;
        };
        let file = self.file(source.unit.file)?;
        let declarations = file.declarations();
        if !source
            .unit
            .matches(file.lowered(), declarations.definitions())
        {
            return None;
        }
        let id = declarations.definition(target.resolved(Some(&source.unit)))?;
        declarations.definitions().resolve(*id).ok()
    }

    /// A category-filtered HIR import must never silently bind a same-name declaration
    /// from another owner (or another kind within the intended owner).
    pub(crate) fn validate_registered_exports(
        &self,
        cancel: &CancellationToken,
    ) -> Result<(), AnalysisError> {
        for metadata in self.native_modules() {
            if metadata.records().exports.is_empty() {
                continue;
            }
            let file = self.registration_file(&metadata.records().identity)?;
            for (key, directive) in named_exports(file) {
                cancel.check()?;
                let expected = metadata.records().exports.get(&key).ok_or_else(|| {
                    DeclarationError(format!("unexpected registered export {}", key.name))
                })?;
                let expected = metadata
                    .definitions()
                    .resolve(*expected)
                    .map_err(|e| DeclarationError(e.to_string()))?;
                let DirectiveResolution::Named(outcomes) = &directive.resolution else {
                    unreachable!("named export")
                };
                if !outcomes[key.namespace]
                    .target()
                    .is_some_and(|target| self.matches_definition(target, expected))
                {
                    return Err(DeclarationError(format!(
                        "{}: export {:?} {} does not resolve to {:?}",
                        metadata.records().identity,
                        key.namespace,
                        key.name,
                        expected.to_path()
                    ))
                    .into());
                }
            }
        }
        Ok(())
    }

    fn publication_exports(
        &self,
        metadata: &DefinitionMetadata<ModuleDecl>,
        cancel: &CancellationToken,
    ) -> Result<Vec<(String, String)>, AnalysisError> {
        let module = metadata.records();
        let file = self.registration_file(&module.identity)?;
        let catalog = &self.module_graph().catalog;
        // A legal leaf contributes either its own category alone or both requested
        // categories. Prefer one dual-category leaf to redundant named imports.
        let mut singles = BTreeMap::<ExportName, String>::new();
        let mut pairs = BTreeMap::<String, String>::new();
        'exports: for (key, directive) in named_exports(file) {
            cancel.check()?;
            let path = &directive.segments;
            if !legal_name(&key.name, cancel)? {
                continue;
            }
            for (i, segment) in path.iter().enumerate() {
                if i == 0 && matches!(segment.as_str(), "crate" | "self" | "super") {
                    continue;
                }
                if !legal_name(segment, cancel)? {
                    continue 'exports;
                }
            }
            let other = match key.namespace {
                NameNamespace::Type => NameNamespace::Value,
                NameNamespace::Value => NameNamespace::Type,
            };
            let ctx = LookupContext::new(&directive.id.unit.module).excluding(directive.id, other);
            let both = match catalog.absolute(&ctx, path, other, cancel)? {
                LookupResult::Missing => false,
                LookupResult::Found(hit) => {
                    let Some(id) = module.exports.get(&ExportName::new(other, &key.name)) else {
                        continue;
                    };
                    let expected = metadata
                        .definitions()
                        .resolve(*id)
                        .map_err(|e| DeclarationError(e.to_string()))?;
                    if !self.matches_definition(&hit.target, expected) {
                        continue;
                    }
                    true
                }
                _ => continue,
            };
            if both {
                pairs.insert(key.name, path.join("::"));
            } else {
                singles.insert(key, path.join("::"));
            }
        }
        let mut exports = Vec::new();
        for key in module.exports.keys() {
            cancel.check()?;
            if let Some(path) = pairs.get(&key.name) {
                if key.namespace == NameNamespace::Type {
                    exports.push((key.name.clone(), path.clone()));
                }
            } else {
                let path = singles
                    .remove(key)
                    .ok_or_else(|| unrepresentable(&module.identity, key))?;
                exports.push((key.name.clone(), path));
            }
        }
        Ok(exports)
    }
}

fn named_exports(file: &FileDeclarations) -> impl Iterator<Item = (ExportName, &ImportDirective)> {
    file.lowered()
        .module
        .imports
        .iter()
        .zip(&file.names().imports.directives)
        .filter_map(|(import, directive)| {
            let ImportLeaf::Named { alias: Some(name) } = &import.kind else {
                return None;
            };
            Some((ExportName::new(import.namespace?, name), directive))
        })
}

fn unrepresentable(owner: &ModuleIdentity, key: &ExportName) -> DeclarationError {
    DeclarationError(format!(
        "{owner}: cannot express export {:?} {} in source without changing its namespace bindings",
        key.namespace, key.name
    ))
}
