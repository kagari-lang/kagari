use crate::{
    error::PackageError,
    identity::{PackageFamily, source_namespace},
    input::InputTree,
    manifest::{DependencySource, Inheritance, MANIFEST_NAME, MAX_MANIFEST_BYTES, Manifest},
    path,
    workspace::{
        WorkspaceInputs, WorkspaceLimits, WorkspacePackage,
        discovery::{self, Discovery},
    },
};
use kagari_common::cancellation::CancellationToken;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct ManifestFile {
    origin: String,
    manifest: Manifest,
}

struct Loader<'a, T> {
    discovery: Discovery<'a, T>,
    manifests: BTreeMap<String, ManifestFile>,
    physical: BTreeMap<String, String>,
}

pub(super) fn load(
    input: &impl InputTree,
    directory: &str,
    namespace: Option<&str>,
    limits: WorkspaceLimits,
    cancel: &CancellationToken,
) -> Result<WorkspaceInputs, PackageError> {
    cancel.check()?;
    let directory = path::join(
        "",
        if directory.is_empty() { "." } else { directory },
        directory,
    )?;
    let mut loader = Loader {
        discovery: Discovery {
            input,
            limits,
            cancel,
            visited: 0,
        },
        manifests: BTreeMap::new(),
        physical: BTreeMap::new(),
    };
    let manifest = loader.require(&directory)?;
    let owner = loader.owner(&directory, &manifest)?;
    let Some(owner) = owner else {
        return loader.standalone(&directory, &manifest, namespace);
    };
    let root = loader.require(&owner)?;
    let configuration = root
        .manifest
        .workspace
        .as_ref()
        .expect("discovered workspace");
    let exclusions = discovery::exclusions(&owner, &configuration.exclude, &root.origin)?;
    if owner != directory && discovery::is_excluded(&directory, &exclusions) {
        if manifest
            .manifest
            .package
            .as_ref()
            .is_some_and(|package| package.workspace.is_some())
        {
            return Err(PackageError::input(
                &manifest.origin,
                "explicit workspace excludes this package",
            ));
        }
        return loader.standalone(&directory, &manifest, namespace);
    }
    let namespace = source_namespace(configuration.namespace.as_deref(), namespace)?;
    let mut pending = loader
        .discovery
        .expand(&owner, &configuration.members, &root.origin)?;
    pending.retain(|directory| !discovery::is_excluded(directory, &exclusions));
    if root.manifest.package.is_some() {
        pending.insert(owner.clone());
    }
    let mut members = BTreeMap::new();
    let mut names = BTreeSet::new();
    while let Some(member_directory) = pending.pop_first() {
        cancel.check()?;
        if members.contains_key(&member_directory) {
            continue;
        }
        let member = loader.require(&member_directory)?;
        if member_directory != owner
            && loader.owner(&member_directory, &member)?.as_deref() != Some(&owner)
        {
            return Err(PackageError::input(
                &member.origin,
                "member belongs to another workspace or needs explicit package.workspace",
            ));
        }
        let package = member.manifest.package.as_ref().ok_or_else(|| {
            PackageError::input(&member.origin, "workspace member must contain [package]")
        })?;
        if package.namespace.is_some() {
            return Err(PackageError::input(
                &member.origin,
                "workspace packages inherit workspace.namespace; remove package.namespace",
            ));
        }
        if !names.insert(package.name.clone()) {
            return Err(PackageError::input(
                &member.origin,
                format!("duplicate workspace package name {}", package.name),
            ));
        }
        let normalized = member
            .manifest
            .normalize(
                &member.origin,
                &member_directory,
                Some(Inheritance {
                    directory: &owner,
                    manifest_origin: &root.origin,
                    workspace: configuration,
                }),
                cancel,
            )?
            .expect("package checked");
        for dependency in normalized.dependencies.values() {
            if let DependencySource::Path(target) = &dependency.source
                && within(target, &owner)
                && !discovery::is_excluded(target, &exclusions)
                && !members.contains_key(target)
            {
                pending.insert(target.clone());
            }
        }
        let family = PackageFamily::new(
            &namespace,
            &path::relative(&owner, &member_directory),
            &normalized.name,
        )?;
        members.insert(
            member_directory,
            WorkspacePackage {
                family,
                manifest: normalized,
                origin: member.origin,
            },
        );
    }
    if directory != owner && !members.contains_key(&directory) {
        return Err(PackageError::input(
            &manifest.origin,
            "package is inside a workspace but is not a member; add it or exclude it",
        ));
    }
    let default_members = if let Some(patterns) = &configuration.default_members {
        let selected = loader.discovery.expand(&owner, patterns, &root.origin)?;
        if let Some(invalid) = selected
            .iter()
            .find(|directory| !members.contains_key(*directory))
        {
            return Err(PackageError::input(
                &root.origin,
                format!("default member {invalid} is not a workspace member"),
            ));
        }
        selected
    } else if root.manifest.package.is_some() {
        [owner.clone()].into()
    } else {
        members.keys().cloned().collect()
    };
    cancel.check()?;
    Ok(WorkspaceInputs {
        directory: owner.clone(),
        namespace,
        members,
        default_members,
        invoking_member: (directory != owner).then_some(directory),
    })
}

impl<T: InputTree> Loader<'_, T> {
    fn read(&mut self, directory: &str) -> Result<Option<ManifestFile>, PackageError> {
        self.discovery.cancel.check()?;
        if let Some(manifest) = self.manifests.get(directory) {
            return Ok(Some(manifest.clone()));
        }
        if self.manifests.len() >= self.discovery.limits.max_manifests {
            return Err(PackageError::input(
                directory,
                "manifest discovery exceeds limit",
            ));
        }
        let name = path::join(directory, MANIFEST_NAME, directory)?;
        let Some(source) =
            self.discovery
                .input
                .read(&name, MAX_MANIFEST_BYTES, self.discovery.cancel)?
        else {
            return Ok(None);
        };
        if let Some(previous) = self.physical.insert(source.key, directory.into()) {
            return Err(PackageError::input(
                &source.origin,
                format!("manifest already loaded through {previous}"),
            ));
        }
        let manifest = Manifest::parse(&source.origin, &source.text, self.discovery.cancel)?;
        let file = ManifestFile {
            origin: source.origin,
            manifest,
        };
        self.manifests.insert(directory.into(), file.clone());
        Ok(Some(file))
    }

    fn require(&mut self, directory: &str) -> Result<ManifestFile, PackageError> {
        self.read(directory)?
            .ok_or_else(|| PackageError::input(directory, "missing Kagari.toml"))
    }

    fn owner(
        &mut self,
        directory: &str,
        file: &ManifestFile,
    ) -> Result<Option<String>, PackageError> {
        if file.manifest.workspace.is_some() {
            return Ok(Some(directory.into()));
        }
        if let Some(explicit) = file
            .manifest
            .package
            .as_ref()
            .and_then(|package| package.workspace.as_deref())
        {
            let owner = path::join(directory, explicit, &file.origin)?;
            if self.require(&owner)?.manifest.workspace.is_none() {
                return Err(PackageError::input(
                    &file.origin,
                    "package.workspace does not name a workspace root",
                ));
            }
            return Ok(Some(owner));
        }
        let mut cursor = directory;
        while !cursor.is_empty() {
            cursor = path::parent(cursor);
            if self
                .read(cursor)?
                .is_some_and(|file| file.manifest.workspace.is_some())
            {
                return Ok(Some(cursor.into()));
            }
        }
        Ok(None)
    }

    fn standalone(
        &self,
        directory: &str,
        file: &ManifestFile,
        supplied: Option<&str>,
    ) -> Result<WorkspaceInputs, PackageError> {
        let package = file
            .manifest
            .package
            .as_ref()
            .ok_or_else(|| PackageError::input(&file.origin, "expected standalone package"))?;
        let namespace = source_namespace(package.namespace.as_deref(), supplied)?;
        let manifest = file
            .manifest
            .normalize(&file.origin, directory, None, self.discovery.cancel)?
            .expect("package checked");
        let family = PackageFamily::new(&namespace, "", &manifest.name)?;
        Ok(WorkspaceInputs {
            directory: directory.into(),
            namespace,
            members: [(
                directory.into(),
                WorkspacePackage {
                    family,
                    manifest,
                    origin: file.origin.clone(),
                },
            )]
            .into(),
            default_members: [directory.into()].into(),
            invoking_member: None,
        })
    }
}

fn within(directory: &str, root: &str) -> bool {
    root.is_empty()
        || directory == root
        || directory
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}
