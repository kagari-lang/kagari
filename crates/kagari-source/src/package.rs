//! Immutable package bindings supplied to source analysis, without acquisition policy.

use kagari_common::identity::PackageId;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// One package's direct import aliases and exact selection identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageScope {
    pub dependencies: BTreeMap<String, PackageId>,
    /// Opaque producer fingerprint for the selected revision, separate from source text.
    pub selection: String,
}

/// Validated source-package scopes retained by snapshots and their query results.
/// Installed native declarations remain a separate explicit compiler input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageEnvironment {
    scopes: BTreeMap<PackageId, PackageScope>,
}

/// A package graph that cannot be retained as a source analysis environment.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PackageEnvironmentError {
    #[error("package environment exceeds package limit")]
    PackageLimitExceeded,
    #[error("package identity must not be empty")]
    EmptyPackageIdentity,
    #[error("package environment exceeds edge limit")]
    EdgeLimitExceeded,
    #[error("invalid package dependency alias {alias:?}")]
    InvalidAlias { alias: String },
    #[error("missing package for dependency {alias}")]
    MissingDependency { alias: String, target: PackageId },
    #[error("package dependency cycle at {}", .package.0)]
    DependencyCycle { package: PackageId },
}

impl PackageEnvironment {
    /// Validates a closed, bounded, acyclic graph of direct source-package bindings.
    /// Missing entries in an empty environment grant no cross-package imports.
    pub fn new(scopes: BTreeMap<PackageId, PackageScope>) -> Result<Self, PackageEnvironmentError> {
        if scopes.len() > 4096 {
            return Err(PackageEnvironmentError::PackageLimitExceeded);
        }
        let mut edge_count = 0usize;
        for (package, scope) in &scopes {
            if package.0.is_empty() {
                return Err(PackageEnvironmentError::EmptyPackageIdentity);
            }
            for (alias, target) in &scope.dependencies {
                edge_count += 1;
                if edge_count > 16384 {
                    return Err(PackageEnvironmentError::EdgeLimitExceeded);
                }
                let mut chars = alias.chars();
                if !chars
                    .next()
                    .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
                    || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    || matches!(alias.as_str(), "_" | "crate" | "self" | "super")
                {
                    return Err(PackageEnvironmentError::InvalidAlias {
                        alias: alias.clone(),
                    });
                }
                if !scopes.contains_key(target) {
                    return Err(PackageEnvironmentError::MissingDependency {
                        alias: alias.clone(),
                        target: target.clone(),
                    });
                }
            }
        }
        let mut complete = BTreeSet::new();
        for root in scopes.keys() {
            let mut active = BTreeSet::new();
            let mut pending = vec![(root, false)];
            while let Some((package, exit)) = pending.pop() {
                if exit {
                    active.remove(package);
                    complete.insert(package);
                    continue;
                }
                if complete.contains(package) {
                    continue;
                }
                if !active.insert(package) {
                    return Err(PackageEnvironmentError::DependencyCycle {
                        package: package.clone(),
                    });
                }
                pending.push((package, true));
                pending.extend(
                    scopes[package]
                        .dependencies
                        .values()
                        .map(|target| (target, false)),
                );
            }
        }
        Ok(Self { scopes })
    }

    pub fn scopes(&self) -> &BTreeMap<PackageId, PackageScope> {
        &self.scopes
    }

    pub fn scope(&self, package: &PackageId) -> Option<&PackageScope> {
        self.scopes.get(package)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_graph_distinguishes_missing_targets_from_cycles() {
        let app = PackageId("app".into());
        let missing = PackageId("missing".into());
        let mut scope = PackageScope {
            dependencies: [("dep".into(), missing.clone())].into(),
            ..Default::default()
        };
        assert_eq!(
            PackageEnvironment::new([(app.clone(), scope.clone())].into()),
            Err(PackageEnvironmentError::MissingDependency {
                alias: "dep".into(),
                target: missing,
            })
        );
        scope.dependencies.insert("dep".into(), app.clone());
        assert_eq!(
            PackageEnvironment::new([(app.clone(), scope)].into()),
            Err(PackageEnvironmentError::DependencyCycle { package: app })
        );
    }
}
