//! Normalize producer inputs into ordinary module import scopes before graph construction.

use crate::{
    analysis::{AnalysisDatabase, core_source, error::AnalysisError},
    imports::{
        LocalName,
        environment::{ImportEnvironment, ModuleScope},
    },
    lower::LoweredModule,
    native::{api, paths::LibraryPaths},
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_source::{package::PackageEnvironment, source::SourceFile};
use kagari_types::declaration::module::DeclarationError;
use std::collections::{BTreeMap, BTreeSet};

struct Producer {
    dependencies: BTreeSet<ModuleIdentity>,
    prelude: bool,
}

impl AnalysisDatabase {
    /// Replaces explicit module import scopes. An entry replaces all defaults for its
    /// importer. Inline children inherit their enclosing scope unless explicitly selected;
    /// separately supplied documents do not inherit by logical path prefix.
    /// Existing snapshots keep their selections. Scopes grant no implementation authority.
    pub fn set_import_environment(&mut self, environment: ImportEnvironment) {
        self.import_environment = environment;
        self.environment_fingerprint.take();
    }

    pub(super) fn import_scopes<'a>(
        &self,
        modules: impl IntoIterator<Item = &'a LoweredModule>,
        packages: &PackageEnvironment,
        paths: &LibraryPaths,
        cancel: &CancellationToken,
    ) -> Result<ImportEnvironment, AnalysisError> {
        let mut declared = BTreeMap::new();
        let mut documents = BTreeMap::new();
        for module in modules {
            cancel.check()?;
            let source = module.source.as_ref();
            declared.insert(source.module_identity(), module);
            documents.insert((source.origin_id(), source.module_identity()), source);
        }
        let mut producers = BTreeMap::new();
        let adopted = self.definitions.snapshot();
        for metadata in &self.native_modules {
            cancel.check()?;
            let input = metadata.records();
            let Some(lowered) = declared.get(&input.identity) else {
                continue;
            };
            let dependencies = api::dependencies(metadata, lowered, &adopted, cancel);
            cancel.check()?;
            producers.insert(
                &input.identity,
                Producer {
                    dependencies: dependencies?,
                    prelude: input.prelude,
                },
            );
        }
        for source in self.core.iter() {
            cancel.check()?;
            let owner = source.module_identity();
            let resource = core_source::resource(owner).expect("bundled core owner");
            producers.insert(
                owner,
                Producer {
                    dependencies: resource
                        .dependencies
                        .iter()
                        .map(|name| core_source::identity(name))
                        .collect(),
                    prelude: false,
                },
            );
        }
        let mut environment = self.import_environment.clone();
        // Within one physical document path order puts a parent before its inline children.
        for (&(origin, identity), source) in &documents {
            cancel.check()?;
            if environment.modules.contains_key(identity) {
                continue;
            }
            let inherited = inline_parent(source)
                .filter(|parent| documents.contains_key(&(origin, parent)))
                .and_then(|parent| environment.scope(&parent));
            let scope = match inherited {
                Some(scope) => scope.clone(),
                None => default_scope(identity, packages, paths, &producers, cancel)?,
            };
            environment.modules.insert(identity.clone(), scope);
        }
        let declared_packages: BTreeSet<_> = declared.keys().map(|id| &id.package).collect();
        for (owner, scope) in &environment.modules {
            cancel.check()?;
            for (alias, target) in &scope.packages.dependencies {
                if LocalName::new(alias).is_none()
                    || matches!(alias.as_str(), "_" | "crate" | "self" | "super")
                {
                    return Err(DeclarationError(format!("invalid package binding {alias}")).into());
                }
                if !declared_packages.contains(target) {
                    return Err(DeclarationError(format!(
                        "missing package {} of {owner}",
                        target.0
                    ))
                    .into());
                }
            }
            for target in scope.dependencies.iter().chain(&scope.preludes) {
                cancel.check()?;
                if !declared.contains_key(target) {
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

fn inline_parent(source: &SourceFile) -> Option<ModuleIdentity> {
    source.inline_range()?;
    let mut parent = source.module_identity().clone();
    parent.path.pop()?;
    Some(parent)
}

fn default_scope(
    identity: &ModuleIdentity,
    packages: &PackageEnvironment,
    paths: &LibraryPaths,
    producers: &BTreeMap<&ModuleIdentity, Producer>,
    cancel: &CancellationToken,
) -> Result<ModuleScope, AnalysisError> {
    let mut scope = ModuleScope {
        packages: packages
            .scope(&identity.package)
            .cloned()
            .unwrap_or_default(),
        ..Default::default()
    };
    let producer = producers.get(identity);
    if let Some(producer) = producer {
        scope.dependencies.clone_from(&producer.dependencies);
    } else {
        for (&owner, producer) in producers {
            cancel.check()?;
            scope.dependencies.insert(owner.clone());
            if producer.prelude {
                scope.preludes.insert(owner.clone());
            }
        }
    }
    // Project defaults expose the supplied libraries. A producer's default spellings
    // cover its own package and the packages its declaration closure selects.
    for (alias, package) in paths.bindings() {
        cancel.check()?;
        if producer.is_some()
            && package != &identity.package
            && !scope.dependencies.iter().any(|id| &id.package == package)
        {
            continue;
        }
        if let Some(previous) = scope
            .packages
            .dependencies
            .insert(alias.clone(), package.clone())
            && &previous != package
        {
            return Err(DeclarationError(format!("conflicting package binding {alias}")).into());
        }
    }
    Ok(scope)
}
