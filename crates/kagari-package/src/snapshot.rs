use crate::{
    error::PackageError,
    graph::{PackageSource, ResolvedGraph},
    host::hash_field,
    identity::{PackageSelection, SourceRevision},
    modules::{ModuleDiscovery, ModuleLimits, ModuleTree, SourceInput},
    path,
};
use blake3::Hasher;
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_source::{
    package::{PackageEnvironment, PackageScope},
    source_database::{CapturedSource, SourceDatabase, SourceSnapshot},
};
use std::collections::BTreeMap;

/// Immutable source/module capture for one exact selection.
#[derive(Debug, Clone)]
pub struct CapturedPackage {
    tree: ModuleTree,
    fingerprint: String,
    dependencies: BTreeMap<String, PackageSelection>,
}

/// One Program's isolated, dependency-complete source inputs.
#[derive(Debug, Clone)]
pub struct ProgramInputs {
    root: PackageSelection,
    packages: BTreeMap<PackageSelection, CapturedPackage>,
    /// Includes exact selections, aliases, module locations and source bytes.
    fingerprint: String,
}

/// Root-local recovery report. Only `program` authorizes complete source handoff.
#[derive(Debug, Clone)]
pub struct ProgramCapture {
    pub program: Option<ProgramInputs>,
    pub discoveries: BTreeMap<PackageSelection, ModuleDiscovery>,
    pub errors: Vec<PackageError>,
}

impl ProgramCapture {
    /// Requires complete capture, preserving the strict compilation boundary.
    pub fn into_complete(self) -> Result<ProgramInputs, PackageError> {
        if let Some(program) = self.program {
            return Ok(program);
        }
        if let Some(error) = self.errors.into_iter().next() {
            return Err(error);
        }
        for discovery in self.discoveries.into_values() {
            discovery.into_complete()?;
        }
        Err(PackageError::input("program", "incomplete source capture"))
    }
}

impl CapturedPackage {
    pub fn tree(&self) -> &ModuleTree {
        &self.tree
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn dependencies(&self) -> &BTreeMap<String, PackageSelection> {
        &self.dependencies
    }
}

impl ProgramInputs {
    /// Registers the complete captured Program and its exact package scopes atomically.
    /// Inline modules are derived by HIR from their containing file, never registered twice.
    pub fn source_snapshot(
        &self,
        sources: &mut SourceDatabase,
        cancel: &CancellationToken,
    ) -> Result<SourceSnapshot, PackageError> {
        cancel.check()?;
        let mut scopes = BTreeMap::new();
        let mut captured = Vec::new();
        for (selection, package) in &self.packages {
            cancel.check()?;
            let id = selection.family.package_id();
            let mut hash = Hasher::new_derive_key("kagari selected package input v1");
            hash_selection(&mut hash, selection);
            scopes.insert(
                id.clone(),
                PackageScope {
                    dependencies: package
                        .dependencies
                        .iter()
                        .map(|(alias, selection)| (alias.clone(), selection.family.package_id()))
                        .collect(),
                    selection: hash.finalize().to_hex().to_string(),
                },
            );
            for (module, location) in &package.tree.modules {
                cancel.check()?;
                if location.inline_range.is_some() {
                    continue;
                }
                let name = format!("package://{}/{}", id.0, module.join("/"));
                let name = format!("{name}/source.kgr");
                let file = &package.tree.files[&location.file];
                captured.push(CapturedSource {
                    key: name,
                    module: ModuleIdentity {
                        package: id.clone(),
                        path: module.clone(),
                    },
                    text: file.text.to_string(),
                    display_name: file.origin.clone(),
                });
            }
        }
        let environment = PackageEnvironment::new(scopes)
            .map_err(|message| PackageError::input("package environment", message))?;
        sources
            .replace_captured(captured, environment, cancel)
            .map_err(|message| {
                if cancel.check().is_err() {
                    PackageError::Cancelled
                } else {
                    PackageError::input("package sources", message)
                }
            })
    }

    pub fn root(&self) -> &PackageSelection {
        &self.root
    }

    pub fn packages(&self) -> &BTreeMap<PackageSelection, CapturedPackage> {
        &self.packages
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

impl ResolvedGraph {
    /// Captures only this root's closure; unrelated members' source bodies are not read.
    /// Module and total-byte budgets apply across the complete Program, not per package.
    pub fn capture(
        &self,
        input: &impl SourceInput,
        root: &PackageSelection,
        limits: ModuleLimits,
        cancel: &CancellationToken,
    ) -> Result<ProgramInputs, PackageError> {
        self.capture_recovering(input, root, limits, cancel)?
            .into_complete()
    }

    /// Retains failed package discovery and continues independent closure inputs.
    /// No incomplete closure can yield `ProgramInputs`; cancellation stops capture.
    pub fn capture_recovering(
        &self,
        input: &impl SourceInput,
        root: &PackageSelection,
        limits: ModuleLimits,
        cancel: &CancellationToken,
    ) -> Result<ProgramCapture, PackageError> {
        let mut discoveries = BTreeMap::new();
        let mut errors = Vec::new();
        let closure = self.closure(root, cancel)?;
        let mut packages = BTreeMap::new();
        let mut hash = Hasher::new_derive_key("kagari program source input v1");
        hash_selection(&mut hash, root);
        let mut physical = BTreeMap::new();
        let mut remaining = limits;
        for selection in closure {
            cancel.check()?;
            let package = &self.packages[&selection];
            let (discovery, directory, local) = match &package.source {
                PackageSource::Local { directory } => (
                    ModuleDiscovery::load(input, &package.manifest.root, remaining, cancel)?,
                    directory.as_str(),
                    true,
                ),
                PackageSource::Host(host) => (
                    ModuleDiscovery::load(
                        &host.sources,
                        &package.manifest.root,
                        remaining,
                        cancel,
                    )?,
                    "",
                    false,
                ),
            };
            remaining.max_modules = remaining
                .max_modules
                .saturating_sub(discovery.modules().len());
            remaining.max_total_bytes = remaining.max_total_bytes.saturating_sub(
                discovery
                    .files()
                    .values()
                    .map(|source| source.text.len())
                    .sum::<usize>(),
            );
            let tree = discovery.clone().into_complete();
            discoveries.insert(selection.clone(), discovery);
            let Ok(tree) = tree else {
                continue;
            };
            if local {
                for source in tree.files.values() {
                    if let Some(previous) =
                        physical.insert(source.key.clone(), selection.family.name().to_owned())
                    {
                        errors.push(PackageError::input(
                            &source.origin,
                            format!(
                                "physical source shared by packages {previous} and {}",
                                selection.family.name()
                            ),
                        ));
                    }
                }
            }
            let mut content = Hasher::new_derive_key("kagari package source snapshot v1");
            hash_field(
                &mut content,
                path::relative(directory, &package.manifest.root).as_bytes(),
            );
            for (file, source) in &tree.files {
                cancel.check()?;
                hash_field(&mut content, path::relative(directory, file).as_bytes());
                content.update(&(source.text.len() as u64).to_le_bytes());
                for chunk in source.text.as_bytes().chunks(64 * 1024) {
                    cancel.check()?;
                    content.update(chunk);
                }
            }
            let fingerprint = content.finalize().to_hex().to_string();
            hash_selection(&mut hash, &selection);
            hash_field(&mut hash, fingerprint.as_bytes());
            hash.update(&(package.dependencies.len() as u64).to_le_bytes());
            for (alias, dependency) in &package.dependencies {
                hash_field(&mut hash, alias.as_bytes());
                hash_selection(&mut hash, dependency);
            }
            packages.insert(
                selection,
                CapturedPackage {
                    tree,
                    fingerprint,
                    dependencies: package.dependencies.clone(),
                },
            );
        }
        cancel.check()?;
        let complete = errors.is_empty() && discoveries.values().all(ModuleDiscovery::is_complete);
        Ok(ProgramCapture {
            program: complete.then(|| ProgramInputs {
                root: root.clone(),
                packages,
                fingerprint: hash.finalize().to_hex().to_string(),
            }),
            discoveries,
            errors,
        })
    }
}

fn hash_selection(hash: &mut Hasher, selection: &PackageSelection) {
    hash_field(hash, selection.family.package_id().0.as_bytes());
    hash_field(hash, selection.version.to_string().as_bytes());
    match &selection.revision {
        SourceRevision::Local => {
            hash_field(hash, b"local");
        }
        SourceRevision::Host(revision) => {
            hash_field(hash, b"host");
            hash_field(hash, revision.as_bytes());
        }
    }
}
