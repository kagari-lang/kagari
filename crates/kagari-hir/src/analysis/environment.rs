//! Normalize producer inputs into ordinary module import scopes before graph construction.

use crate::{
    analysis::{AnalysisDatabase, error::AnalysisError},
    imports::{
        LocalName,
        environment::{ImportEnvironment, ModuleScope},
    },
    lower::LoweredModule,
    native::api,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::package::PackageEnvironment;
use kagari_types::declaration::module::DeclarationError;
use std::{collections::BTreeSet, iter};

impl AnalysisDatabase {
    /// Replaces explicit module import scopes. Inputs contain no implementation authority.
    /// Existing snapshots keep their selections; future queries rebuild semantic facts.
    pub fn set_import_environment(&mut self, environment: ImportEnvironment) {
        self.import_environment = environment;
        self.environment_fingerprint.take();
        self.declaration_cache = None;
        self.signature_cache = None;
        self.body_cache.clear();
        self.files.clear();
    }

    pub(super) fn import_scopes<'a>(
        &self,
        modules: impl IntoIterator<Item = &'a LoweredModule>,
        packages: &PackageEnvironment,
        cancel: &CancellationToken,
    ) -> Result<ImportEnvironment, AnalysisError> {
        let modules = modules.into_iter().collect::<Vec<_>>();
        let declared = modules
            .iter()
            .map(|module| module.source.module_identity().clone())
            .collect::<BTreeSet<_>>();
        let mut libraries = Vec::new();
        for metadata in &self.native_modules {
            cancel.check()?;
            let input = metadata.records();
            let Some(lowered) = modules
                .iter()
                .find(|module| module.source.module_identity() == &input.identity)
            else {
                continue;
            };
            libraries.push((
                input.identity.clone(),
                input.package_alias.clone(),
                input.prelude,
                api::dependencies(metadata, &self.native_modules, lowered, cancel)?,
            ));
        }
        for input in &self.external_modules {
            libraries.push((
                input.source.module_identity().clone(),
                input.package_alias.clone(),
                input.prelude,
                input.dependencies.clone(),
            ));
        }
        let mut environment = self.import_environment.clone();
        for module in modules {
            cancel.check()?;
            let identity = module.source.module_identity();
            if environment.modules.contains_key(identity) {
                continue;
            }
            let producer = libraries
                .iter()
                .find(|(owner, ..)| owner == identity)
                .or_else(|| {
                    libraries
                        .iter()
                        .filter(|(owner, ..)| {
                            owner.package == identity.package
                                && identity.path.starts_with(&owner.path)
                        })
                        .max_by_key(|(owner, ..)| owner.path.len())
                });
            let mut scope = ModuleScope {
                packages: packages
                    .scope(&identity.package)
                    .cloned()
                    .unwrap_or_default(),
                ..Default::default()
            };
            for (owner, alias, prelude, _) in &libraries {
                for alias in alias
                    .iter()
                    .chain(iter::once(&owner.package.0))
                    .filter(|alias| LocalName::new(alias).is_some())
                {
                    if let Some(previous) = scope
                        .packages
                        .dependencies
                        .insert(alias.clone(), owner.package.clone())
                        && previous != owner.package
                    {
                        return Err(DeclarationError(format!(
                            "conflicting package binding {alias}"
                        ))
                        .into());
                    }
                }
                if producer.is_none() {
                    scope.dependencies.insert(owner.clone());
                    if *prelude {
                        scope.preludes.insert(owner.clone());
                    }
                }
            }
            if let Some((_, _, _, dependencies)) = producer {
                scope.dependencies.extend(dependencies.iter().cloned());
            }
            environment.modules.insert(identity.clone(), scope);
        }
        for (owner, scope) in &environment.modules {
            cancel.check()?;
            for (alias, target) in &scope.packages.dependencies {
                if LocalName::new(alias).is_none()
                    || matches!(alias.as_str(), "_" | "crate" | "self" | "super")
                {
                    return Err(DeclarationError(format!("invalid package binding {alias}")).into());
                }
                if !declared.iter().any(|module| &module.package == target) {
                    return Err(DeclarationError(format!(
                        "missing package {} of {owner}",
                        target.0
                    ))
                    .into());
                }
            }
            for target in scope.dependencies.iter().chain(&scope.preludes) {
                cancel.check()?;
                if !declared.contains(target) {
                    return Err(DeclarationError(format!(
                        "missing dependency {target} of {owner}"
                    ))
                    .into());
                }
            }
        }
        Ok(environment)
    }
}
