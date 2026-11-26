//! Source spellings of explicit library owners, independent of registration authority.
use crate::{
    analysis::core_source,
    imports::{
        LocalName,
        environment::{ImportEnvironment, ModuleScope, package_targets, source_package_alias},
    },
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{ModuleIdentity, PackageId, metadata::DefinitionMetadata, table::DefinitionView},
};
use kagari_source::{
    package::{PackageEnvironment, PackageScope},
    source::SourceFile,
};
use kagari_syntax::{lexer::lex_with_cancellation, token::TokenKind};
use kagari_types::declaration::module::{DeclarationError, ModuleDecl};
use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
    sync::Arc,
};

/// Presentation and import-scope inputs only. Duplicate owners are rejected by
/// declaration preparation; an alias never creates a module or grants visibility.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct LibraryPaths {
    explicit: BTreeMap<ModuleIdentity, ModuleScope>,
    package_scopes: BTreeMap<PackageId, PackageScope>,
    preferred: BTreeMap<ModuleIdentity, String>,
    packages: BTreeMap<PackageId, BTreeSet<String>>,
}

impl LibraryPaths {
    pub(crate) fn new(
        native: &[Arc<DefinitionMetadata<ModuleDecl>>],
        core: &[Arc<SourceFile>],
    ) -> Self {
        let native = native.iter().map(|provider| {
            let module = provider.records();
            (&module.identity, module.package_alias.as_deref())
        });
        let authored = core.iter().map(|source| {
            let owner = source.module_identity();
            (owner, core_source::resource(owner).map(|m| m.package_alias))
        });
        let mut paths = Self::default();
        for (owner, alias) in native.chain(authored) {
            let aliases = paths.packages.entry(owner.package.clone()).or_default();
            if let Some(alias) = alias.filter(|name| LocalName::new(name).is_some()) {
                aliases.insert(alias.to_owned());
                paths.preferred.insert(owner.clone(), alias.to_owned());
            }
        }
        paths
    }

    /// Keep only request selections that can affect a prepared registration.
    pub(crate) fn with_importers(
        mut self,
        native: &[Arc<DefinitionMetadata<ModuleDecl>>],
        environment: &ImportEnvironment,
        packages: &PackageEnvironment,
    ) -> Self {
        for module in native {
            let id = &module.records().identity;
            if let Some(scope) = environment.scope(id) {
                self.explicit.insert(id.clone(), scope.clone());
            } else if let Some(scope) = packages.scope(&id.package) {
                self.package_scopes
                    .insert(id.package.clone(), scope.clone());
            }
        }
        self
    }

    pub(crate) fn bindings(&self) -> impl Iterator<Item = (&String, &PackageId)> {
        self.packages.iter().flat_map(|(package, aliases)| {
            let canonical = LocalName::new(&package.0).map(|_| &package.0);
            aliases
                .iter()
                .chain(canonical)
                .map(move |name| (name, package))
        })
    }

    pub(crate) fn importer<'a>(&'a self, importer: &'a ModuleIdentity) -> ImporterPaths<'a> {
        match self.explicit.get(importer) {
            Some(scope) => ImporterPaths::effective(importer, scope),
            None => ImporterPaths {
                importer,
                packages: self.package_scopes.get(&importer.package),
                defaults: Some(self),
                scope: None,
            },
        }
    }
}

/// A replacement scope never falls through to producer-advertised aliases. Before
/// graph construction, a default producer may also use its selected library's names;
/// the graph normalizer checks conflicting default bindings. Publication uses only
/// the frozen effective scope (including dependency spellings permitted by lookup).
#[derive(Clone, Copy)]
pub(crate) struct ImporterPaths<'a> {
    importer: &'a ModuleIdentity,
    packages: Option<&'a PackageScope>,
    defaults: Option<&'a LibraryPaths>,
    scope: Option<&'a ModuleScope>,
}

impl<'a> ImporterPaths<'a> {
    pub(crate) fn effective(importer: &'a ModuleIdentity, scope: &'a ModuleScope) -> Self {
        Self {
            importer,
            packages: Some(&scope.packages),
            defaults: None,
            scope: Some(scope),
        }
    }

    pub(crate) fn definition_path(
        self,
        target: DefinitionView<'_>,
        cancel: &CancellationToken,
    ) -> Result<String, DeclarationError> {
        let mut path = self.module_path(target.module(), cancel)?;
        for segment in target.segments() {
            path.push_str("::");
            path.push_str(segment.name);
        }
        Ok(path)
    }

    pub(crate) fn module_path(
        self,
        identity: &ModuleIdentity,
        cancel: &CancellationToken,
    ) -> Result<String, DeclarationError> {
        self.module_path_matching(identity, cancel, |_| Ok(true))
    }

    /// A valid package binding can still be shadowed by a module-level name in a
    /// type expression. Publication checks the whole written path before choosing it.
    pub(super) fn module_path_matching(
        self,
        identity: &ModuleIdentity,
        cancel: &CancellationToken,
        accepts: impl Fn(&str) -> Result<bool, DeclarationError>,
    ) -> Result<String, DeclarationError> {
        let cancelled = |_: Cancelled| DeclarationError("native path spelling cancelled".into());
        let candidate = |package: &str| -> Result<Option<String>, DeclarationError> {
            let path = iter::once(package)
                .chain(identity.path.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join("::");
            Ok(accepts(&path)?.then_some(path))
        };
        let importer = self.importer;
        for (name, target) in self
            .packages
            .into_iter()
            .flat_map(|scope| &scope.dependencies)
        {
            cancel.check().map_err(cancelled)?;
            if *target == identity.package
                && legal_name(name, cancel).map_err(cancelled)?
                && let Some(path) = candidate(name)?
            {
                return Ok(path);
            }
        }
        if let Some(defaults) = self.defaults {
            for name in defaults.preferred.get(identity).into_iter().chain(
                defaults
                    .packages
                    .get(&identity.package)
                    .into_iter()
                    .flatten(),
            ) {
                if legal_name(name, cancel).map_err(cancelled)?
                    && let Some(path) = candidate(name)?
                {
                    return Ok(path);
                }
            }
        }
        cancel.check().map_err(cancelled)?;
        let fallback = source_package_alias(&identity.package);
        let fallback = fallback.as_deref().unwrap_or(&identity.package.0);
        let package = if identity.package == importer.package {
            "crate"
        } else if legal_name(fallback, cancel).map_err(cancelled)?
            && !self
                .packages
                .is_some_and(|scope| scope.dependencies.contains_key(fallback))
            && self.scope.is_none_or(|scope| {
                let targets = package_targets(Some(scope), importer, fallback);
                targets.len() == 1 && targets.contains(&identity.package)
            })
        {
            fallback
        } else {
            return Err(DeclarationError(format!(
                "no package spelling for {identity} from {importer}"
            )));
        };
        candidate(package)?.ok_or_else(|| {
            DeclarationError(format!(
                "no accessible package spelling for {identity} from {importer}"
            ))
        })
    }
}

pub(super) fn legal_name(name: &str, cancel: &CancellationToken) -> Result<bool, Cancelled> {
    let tokens = lex_with_cancellation(name, cancel)?;
    Ok(name != "_" && tokens.len() == 2 && tokens[0].kind == TokenKind::Ident)
}
