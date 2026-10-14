use crate::{
    error::PackageError,
    manifest::{
        Dependency, DependencyRequirement, DependencySource, Inheritance, Manifest, PackageManifest,
    },
    path,
};
use kagari_common::cancellation::CancellationToken;
use kagari_syntax::{lexer::lex, token::TokenKind};
use std::collections::BTreeMap;

impl Manifest {
    pub(super) fn validate(&self, origin: &str) -> Result<(), PackageError> {
        if self.package.is_none() && self.workspace.is_none() {
            return Err(PackageError::input(
                origin,
                "expected [package] or [workspace]",
            ));
        }
        if self.package.is_none() && !self.dependencies.is_empty() {
            return Err(PackageError::input(
                origin,
                "virtual workspaces cannot declare [dependencies]",
            ));
        }
        if let Some(package) = &self.package {
            validate_name(&package.name, origin, true)?;
            if self.workspace.is_some() && package.workspace.is_some() {
                return Err(PackageError::input(
                    origin,
                    "a workspace root cannot select another workspace",
                ));
            }
        }
        for (alias, dependency) in &self.dependencies {
            validate_dependency(alias, dependency, origin, true)?;
        }
        if let Some(workspace) = &self.workspace {
            for (alias, dependency) in &workspace.dependencies {
                validate_dependency(alias, dependency, origin, false)?;
            }
        }
        Ok(())
    }

    /// Resolves explicit inheritance and paths without touching a filesystem.
    ///
    /// `directory` is this manifest's directory within an approved input root.
    /// Inherited paths use the workspace directory; local paths use `directory`.
    /// Missing inheritance, invalid aliases and paths escaping the input root fail.
    /// A virtual workspace returns `None`.
    pub fn normalize(
        &self,
        origin: &str,
        directory: &str,
        inheritance: Option<Inheritance<'_>>,
        cancel: &CancellationToken,
    ) -> Result<Option<PackageManifest>, PackageError> {
        cancel.check()?;
        self.validate(origin)?;
        let Some(package) = &self.package else {
            return Ok(None);
        };
        let root = path::join(directory, &package.root, origin)?;
        if !root.ends_with(".kgr") {
            return Err(PackageError::input(
                origin,
                "package root must be a .kgr source",
            ));
        }
        let mut dependencies = BTreeMap::new();
        for (alias, dependency) in &self.dependencies {
            cancel.check()?;
            let (declaration, base, declaration_origin) = if dependency.workspace == Some(true) {
                let inherited = inheritance.as_ref().ok_or_else(|| {
                    PackageError::input(
                        origin,
                        format!("dependency {alias} requires an owning workspace"),
                    )
                })?;
                let declaration = inherited.workspace.dependencies.get(alias).ok_or_else(|| {
                    PackageError::input(origin, format!("missing workspace dependency {alias}"))
                })?;
                validate_dependency(alias, declaration, inherited.manifest_origin, false)?;
                (declaration, inherited.directory, inherited.manifest_origin)
            } else {
                (dependency, directory, origin)
            };
            let origin = format!("{declaration_origin}: dependency {alias}");
            let source = match (&declaration.path, &declaration.host) {
                (Some(target), None) => DependencySource::Path(path::join(base, target, &origin)?),
                (None, Some(key)) => DependencySource::Host(key.clone()),
                _ => {
                    return Err(PackageError::input(
                        &origin,
                        "expected exactly one path or host source",
                    ));
                }
            };
            dependencies.insert(
                alias.clone(),
                DependencyRequirement {
                    package: declaration.package.clone().unwrap_or_else(|| alias.clone()),
                    version: declaration.version.clone(),
                    source,
                    origin,
                },
            );
        }
        cancel.check()?;
        Ok(Some(PackageManifest {
            name: package.name.clone(),
            version: package.version.clone(),
            root,
            dependencies,
        }))
    }
}

fn validate_dependency(
    alias: &str,
    dependency: &Dependency,
    origin: &str,
    allow_inheritance: bool,
) -> Result<(), PackageError> {
    validate_name(alias, origin, false)?;
    if let Some(name) = &dependency.package {
        validate_name(name, origin, true)?;
    }
    let inherited = dependency.workspace == Some(true);
    let has_fields = dependency.package.is_some()
        || dependency.version.is_some()
        || dependency.path.is_some()
        || dependency.host.is_some();
    if dependency.workspace == Some(false) || (inherited && (!allow_inheritance || has_fields)) {
        return Err(PackageError::input(
            origin,
            format!(
                "dependency {alias}: workspace inheritance must be exactly {{ workspace = true }}"
            ),
        ));
    }
    if inherited {
        return Ok(());
    }
    if dependency.path.is_some() == dependency.host.is_some() {
        return Err(PackageError::input(
            origin,
            format!(
                "dependency {alias}: expected exactly one path or host source; remote acquisition is unsupported"
            ),
        ));
    }
    if dependency.host.as_ref().is_some_and(|key| key.is_empty()) {
        return Err(PackageError::input(
            origin,
            format!("dependency {alias}: empty host key"),
        ));
    }
    Ok(())
}

fn validate_name(name: &str, origin: &str, package: bool) -> Result<(), PackageError> {
    let mut chars = name.chars();
    let valid = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || (package && c == '-'));
    let identifier = package
        || lex(name)
            .first()
            .is_some_and(|token| token.kind == TokenKind::Ident);
    if !valid || name == "_" || !identifier {
        return Err(PackageError::input(
            origin,
            format!(
                "invalid {} name {name:?}",
                if package {
                    "package"
                } else {
                    "dependency alias"
                }
            ),
        ));
    }
    Ok(())
}
