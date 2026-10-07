//! Snapshot-owned, unfiltered namespaces. Targets never carry member tables.
use crate::{
    host::HostDeclarations,
    imports::{
        BindingCandidate, BindingOrigin, NamespaceId, ResolvedTarget, SourceDeclRef, SourceUnit,
        cache::same_name_tables,
    },
    resolver::{resolved::ResolvedName, table::NameTable},
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{ModuleIdentity, PackageId},
};
use kagari_types::declaration::names::NameNamespace;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    iter,
    sync::Arc,
};

/// Unfiltered member candidates for one namespace and its glob-expansion policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceTable {
    /// Logical owner used when constructing/accessing this namespace.
    pub owner: ModuleIdentity,
    /// Shared tiered candidates; visibility is checked against a lookup context.
    pub names: Arc<NameTable>,
    /// Whether this namespace permits `use ...::*` expansion.
    pub glob_allowed: bool,
}

/// Canonical namespace lookup data retained by one module graph/snapshot.
///
/// ```text
/// scope.hit("m") -> Found(namespace target)
/// namespace_of(target) -> NamespaceId
/// lookup_member(ctx, namespace, "nested") -> Found(next target)
/// namespace_of(next target) -> next NamespaceId
/// lookup_member(ctx, next namespace, "value") -> canonical declaration
/// ```
///
/// [`Self::resolve_path`] performs this walk and accumulates binding origins.
/// Tables retain private candidates as well as public ones: lookup checks access
/// at each component. Source references must still belong to this catalog. A target
/// stores only identity, so aliases do not copy member tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamespaceCatalog {
    /// All exact source units grouped by logical module identity; multiple entries remain ambiguous.
    pub(crate) modules: BTreeMap<ModuleIdentity, Vec<SourceUnit>>,
    /// Namespace identity to owner, shared name table and glob policy.
    pub(crate) namespaces: HashMap<NamespaceId, NamespaceTable>,
    /// Installed package aliases; multiple package targets make an alias ambiguous.
    pub(crate) package_aliases: BTreeMap<String, BTreeSet<PackageId>>,
}

/// A canonical target plus the selected binding origins along its lookup path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupHit {
    /// Category selected at this source site.
    pub namespace: NameNamespace,
    /// Destination identity, independent of the import spelling.
    pub target: ResolvedTarget,
    /// Origins retained in traversal order, including equal-target glob contributions.
    pub via: Vec<BindingOrigin>,
}

/// Detailed lookup outcome, preserving failure distinctions for diagnostics and tooling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupResult {
    /// One target with its selected provenance.
    Found(LookupHit),
    /// No binding with that spelling exists.
    Missing,
    /// A binding exists but has no resolved target; weaker tiers stay hidden.
    Unresolved,
    /// Selected candidates conflict; the list may be empty for a package/module collision.
    Ambiguous(Vec<BindingCandidate>),
    /// Candidates exist but are not accessible from the importer.
    Inaccessible(Vec<BindingCandidate>),
    /// The selected target cannot be entered as a member container.
    NotNamespace,
    /// A source unit does not belong to the retained catalog.
    StaleSource,
}

impl LookupResult {
    /// Extracts a unique hit, discarding all failure distinctions.
    pub(crate) fn hit(self) -> Option<LookupHit> {
        if let Self::Found(hit) = self {
            Some(hit)
        } else {
            None
        }
    }
}

/// Whether a target can be entered for further member lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamespaceResult {
    /// Namespace identity to use for the next component.
    Found(NamespaceId),
    /// No member namespace exists for this target.
    NotNamespace,
    /// The source target no longer belongs to this catalog.
    StaleSource,
}

/// Importer-relative access context supplied without mutating catalog tables.
pub struct LookupContext<'a> {
    /// Logical module requesting access.
    pub importer: &'a ModuleIdentity,
    /// Installed host namespace/type/function declarations.
    pub hosts: &'a HostDeclarations,
}

impl NamespaceCatalog {
    pub(crate) fn valid(&self, unit: &SourceUnit) -> bool {
        self.modules
            .get(&unit.module)
            .is_some_and(|units| units.contains(unit))
    }

    /// Selects one member, checking source validity, precedence and importer visibility.
    ///
    /// Host namespaces delegate to host declarations. Lookup failures are returned as
    /// [`LookupResult`] variants, not as cancellation errors.
    ///
    /// # Errors
    ///
    /// Returns `Cancelled` when the cancellation token is set.
    pub fn lookup_member(
        &self,
        ctx: &LookupContext<'_>,
        ns: &NamespaceId,
        name: &str,
        namespace: NameNamespace,
        cancel: &CancellationToken,
    ) -> Result<LookupResult, Cancelled> {
        cancel.check()?;
        if let NamespaceId::Host(id) = ns {
            return Ok(ctx
                .hosts
                .resolve_name_in(*id, name, namespace)
                .and_then(host_target)
                .map_or(LookupResult::Missing, |target| {
                    LookupResult::Found(LookupHit {
                        namespace,
                        target,
                        via: vec![],
                    })
                }));
        }
        if let NamespaceId::Module(unit) = ns
            && !self.valid(unit)
        {
            return Ok(LookupResult::StaleSource);
        }
        if let NamespaceId::Associated(source) = ns
            && !self.valid(&source.unit)
        {
            return Ok(LookupResult::StaleSource);
        }
        let Some(table) = self.namespaces.get(ns) else {
            return Ok(LookupResult::NotNamespace);
        };
        let Some((candidates, strong)) = table.names.candidates(name, namespace) else {
            if let NamespaceId::InstalledPrefix(identity) = ns {
                let aliases = iter::once(identity.package.0.as_str()).chain(
                    self.package_aliases
                        .iter()
                        .filter(|(_, packages)| {
                            packages.len() == 1 && packages.contains(&identity.package)
                        })
                        .map(|(alias, _)| alias.as_str()),
                );
                for alias in aliases {
                    {
                        let path = iter::once(alias)
                            .chain(identity.path.iter().map(String::as_str))
                            .chain(iter::once(name))
                            .collect::<Vec<_>>()
                            .join("::");
                        if let Some(target) = ctx
                            .hosts
                            .resolve_name(&path, namespace)
                            .and_then(host_target)
                        {
                            return Ok(LookupResult::Found(LookupHit {
                                namespace,
                                target,
                                via: vec![],
                            }));
                        }
                    }
                }
            }
            return Ok(LookupResult::Missing);
        };
        if strong && candidates.len() > 1 {
            return Ok(LookupResult::Ambiguous(candidates.to_vec()));
        }
        let admitted = candidates
            .iter()
            .filter(|c| c.visibility.allows(&c.owner, ctx.importer))
            .cloned()
            .collect::<Vec<_>>();
        if admitted.is_empty() {
            return Ok(LookupResult::Inaccessible(candidates.to_vec()));
        }
        Ok(NameTable::select(&admitted, strong))
    }

    /// Obtains a target's member-container identity, checking qualified source units.
    ///
    /// This does not itself select or authorize any member; [`Self::lookup_member`]
    /// performs member access checks.
    ///
    /// # Errors
    ///
    /// Returns `Cancelled` if cancellation is observed.
    pub fn namespace_of(
        &self,
        _ctx: &LookupContext<'_>,
        target: &ResolvedTarget,
        cancel: &CancellationToken,
    ) -> Result<NamespaceResult, Cancelled> {
        cancel.check()?;
        Ok(match target {
            ResolvedTarget::Namespace(ns) => match ns {
                NamespaceId::Module(unit) if !self.valid(unit) => NamespaceResult::StaleSource,
                NamespaceId::Associated(source) if !self.valid(&source.unit) => {
                    NamespaceResult::StaleSource
                }
                _ => NamespaceResult::Found(ns.clone()),
            },
            ResolvedTarget::Source(source) if !self.valid(&source.unit) => {
                NamespaceResult::StaleSource
            }
            ResolvedTarget::Source(source) => {
                let ns = NamespaceId::Associated(source.clone());
                if self.namespaces.contains_key(&ns) {
                    NamespaceResult::Found(ns)
                } else {
                    NamespaceResult::NotNamespace
                }
            }
            _ => NamespaceResult::NotNamespace,
        })
    }

    /// Walks `::`-separated suffix components from an existing root lookup.
    ///
    /// Empty components are skipped. A failed root/component stops traversal unchanged;
    /// successful steps append provenance. No local bindings or import records are added.
    ///
    /// # Errors
    ///
    /// Returns `Cancelled` if cancellation is observed during traversal.
    pub fn resolve_path(
        &self,
        ctx: &LookupContext<'_>,
        mut root: LookupResult,
        suffix: &str,
        namespace: NameNamespace,
        cancel: &CancellationToken,
    ) -> Result<LookupResult, Cancelled> {
        let mut components = suffix.split("::").filter(|c| !c.is_empty()).peekable();
        while let Some(component) = components.next() {
            let category = if components.peek().is_some() {
                NameNamespace::Type
            } else {
                namespace
            };
            cancel.check()?;
            let LookupResult::Found(previous) = root else {
                return Ok(root);
            };
            let ns = match self.namespace_of(ctx, &previous.target, cancel)? {
                NamespaceResult::Found(ns) => ns,
                NamespaceResult::NotNamespace => return Ok(LookupResult::NotNamespace),
                NamespaceResult::StaleSource => return Ok(LookupResult::StaleSource),
            };
            root = self.lookup_member(ctx, &ns, component, category, cancel)?;
            if let LookupResult::Found(hit) = &mut root {
                let mut via = previous.via;
                via.append(&mut hit.via);
                hit.via = via;
            }
        }
        Ok(root)
    }

    pub(crate) fn absolute(
        &self,
        ctx: &LookupContext<'_>,
        path: &str,
        namespace: NameNamespace,
        cancel: &CancellationToken,
    ) -> Result<LookupResult, Cancelled> {
        cancel.check()?;
        let segments = path.split("::").collect::<Vec<_>>();
        let Some(package) = segments.first() else {
            return Ok(LookupResult::Missing);
        };
        let packages = self
            .package_aliases
            .get(*package)
            .cloned()
            .unwrap_or_else(|| BTreeSet::from([PackageId((*package).into())]));
        if packages.len() > 1 {
            return Ok(LookupResult::Ambiguous(vec![]));
        }
        let package = packages.first().expect("package");
        for end in 1..segments.len() {
            let identity = ModuleIdentity {
                package: package.clone(),
                path: segments[1..=end].iter().map(|s| (*s).into()).collect(),
            };
            if let Some(units) = self.modules.get(&identity) {
                if end + 1 == segments.len() && namespace == NameNamespace::Value {
                    return Ok(LookupResult::Missing);
                }
                if units.len() != 1 {
                    return Ok(LookupResult::Ambiguous(vec![]));
                }
                if !self.module_accessible(ctx, &identity, cancel)? {
                    return Ok(LookupResult::Inaccessible(vec![]));
                }
                let root = LookupResult::Found(LookupHit {
                    namespace: NameNamespace::Type,
                    target: ResolvedTarget::Namespace(NamespaceId::Module(units[0].clone())),
                    via: vec![],
                });
                let result = self.resolve_path(
                    ctx,
                    root,
                    &segments[end + 1..].join("::"),
                    namespace,
                    cancel,
                )?;
                let terminal = ModuleIdentity {
                    package: package.clone(),
                    path: segments[1..].iter().map(|s| (*s).to_owned()).collect(),
                };
                let physical_collision = namespace == NameNamespace::Type && self.modules.get(&terminal).is_some_and(|units| {
                    matches!(&result, LookupResult::Found(hit) if !matches!(&hit.target, ResolvedTarget::Namespace(NamespaceId::Module(unit)) if units.as_slice() == [unit.clone()]))
                });
                return Ok(
                    if physical_collision || ctx.hosts.resolve_name(path, namespace).is_some() {
                        LookupResult::Ambiguous(vec![])
                    } else {
                        result
                    },
                );
            }
        }
        if let Some(target) = ctx
            .hosts
            .resolve_name(path, namespace)
            .and_then(host_target)
        {
            return Ok(LookupResult::Found(LookupHit {
                namespace,
                target,
                via: vec![],
            }));
        }
        let prefix = NamespaceId::InstalledPrefix(ModuleIdentity {
            package: package.clone(),
            path: vec![],
        });
        if self.namespaces.contains_key(&prefix) {
            return self.resolve_path(
                ctx,
                LookupResult::Found(LookupHit {
                    namespace: NameNamespace::Type,
                    target: ResolvedTarget::Namespace(prefix),
                    via: vec![],
                }),
                &segments[1..].join("::"),
                namespace,
                cancel,
            );
        }
        Ok(LookupResult::Missing)
    }

    fn module_accessible(
        &self,
        ctx: &LookupContext<'_>,
        identity: &ModuleIdentity,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        let mut parent = identity.clone();
        while parent.path.len() > 1 {
            cancel.check()?;
            let name = parent.path.pop().expect("child");
            if let Some(units) = self.modules.get(&parent) {
                for unit in units {
                    if matches!(
                        self.lookup_member(
                            ctx,
                            &NamespaceId::Module(unit.clone()),
                            &name,
                            NameNamespace::Type,
                            cancel
                        )?,
                        LookupResult::Inaccessible(_)
                    ) {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    /// Collects foreign declarations reachable through scope bindings and accessible namespaces, guarding namespace cycles.
    pub(crate) fn reachable_sources(
        &self,
        names: &NameTable,
        hosts: &HostDeclarations,
        cancel: &CancellationToken,
    ) -> Result<HashSet<SourceDeclRef>, Cancelled> {
        let Some(unit) = names.unit.as_ref() else {
            return Ok(HashSet::new());
        };
        let ctx = LookupContext {
            importer: &unit.module,
            hosts,
        };
        let mut pending = names
            .entries
            .keys()
            .flat_map(|name| {
                NameNamespace::ALL.into_iter().filter_map(move |namespace| {
                    names
                        .hit(name.as_str(), namespace)
                        .hit()
                        .map(|hit| hit.target)
                })
            })
            .collect::<Vec<_>>();
        let mut seen = HashSet::new();
        let mut sources = HashSet::new();
        while let Some(target) = pending.pop() {
            cancel.check()?;
            if let ResolvedTarget::Source(source) = &target
                && source.unit != *unit
            {
                sources.insert(source.clone());
            }
            let NamespaceResult::Found(ns) = self.namespace_of(&ctx, &target, cancel)? else {
                continue;
            };
            if !seen.insert(ns.clone()) {
                continue;
            }
            if let Some(table) = self.namespaces.get(&ns) {
                for name in table.names.entries.keys() {
                    for namespace in NameNamespace::ALL {
                        if let Some(hit) = self
                            .lookup_member(&ctx, &ns, name.as_str(), namespace, cancel)?
                            .hit()
                        {
                            pending.push(hit.target);
                        }
                    }
                }
            }
        }
        Ok(sources)
    }

    /// Resolves a scope root and suffix, falling back to an absolute path only when the root is missing; returns only unique hits.
    pub(crate) fn resolve_name(
        &self,
        names: &NameTable,
        hosts: &HostDeclarations,
        name: &str,
        namespace: NameNamespace,
        cancel: &CancellationToken,
    ) -> Option<LookupHit> {
        let unit = names.unit.as_ref()?;
        let ctx = LookupContext {
            importer: &unit.module,
            hosts,
        };
        let (root, suffix) = name.split_once("::").unwrap_or((name, ""));
        let result = names.hit(
            root,
            if suffix.is_empty() {
                namespace
            } else {
                NameNamespace::Type
            },
        );
        let result = if matches!(result, LookupResult::Missing) {
            self.absolute(&ctx, name, namespace, cancel).ok()?
        } else {
            self.resolve_path(&ctx, result, suffix, namespace, cancel)
                .ok()?
        };
        result.hit()
    }
}

fn host_target(name: ResolvedName) -> Option<ResolvedTarget> {
    Some(match name {
        ResolvedName::HostModule(id) => ResolvedTarget::Namespace(NamespaceId::Host(id)),
        ResolvedName::HostFunction(id) => ResolvedTarget::HostFunction(id),
        ResolvedName::HostType(id) => ResolvedTarget::HostType(id),
        _ => return None,
    })
}

impl NamespaceCatalog {
    /// A retained file may keep its catalog only when every namespace it can
    /// enter still has the same qualified targets, candidates and access facts.
    pub(crate) fn same_namespaces(
        &self,
        other: &Self,
        names: &NameTable,
        hosts: &HostDeclarations,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        self.same_namespace_inputs(other, names, None, hosts, cancel)
    }

    /// Reuse checked facts across a local lowering only after comparing all
    /// reachable member bindings. The existing remappers handle local arena IDs;
    /// foreign namespace identities and targets must still match exactly.
    pub(crate) fn same_reuse_namespaces(
        &self,
        other: &Self,
        names: &NameTable,
        other_names: &NameTable,
        hosts: &HostDeclarations,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        let (Some(unit), Some(other_unit)) = (&names.unit, &other_names.unit) else {
            return Ok(false);
        };
        if unit.module != other_unit.module || unit.file != other_unit.file {
            return Ok(false);
        }
        self.same_namespace_inputs(other, names, Some(other_unit), hosts, cancel)
    }

    fn same_namespace_inputs(
        &self,
        other: &Self,
        names: &NameTable,
        rebased_unit: Option<&SourceUnit>,
        hosts: &HostDeclarations,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        let Some(unit) = names.unit.as_ref() else {
            return Ok(false);
        };
        let ctx = LookupContext {
            importer: &unit.module,
            hosts,
        };
        let mut pending = names
            .entries
            .keys()
            .flat_map(|name| {
                NameNamespace::ALL.into_iter().filter_map(move |namespace| {
                    names
                        .hit(name.as_str(), namespace)
                        .hit()
                        .map(|hit| hit.target)
                })
            })
            .collect::<Vec<_>>();
        let mut seen = HashSet::new();
        while let Some(target) = pending.pop() {
            cancel.check()?;
            let NamespaceResult::Found(ns) = self.namespace_of(&ctx, &target, cancel)? else {
                continue;
            };
            if !seen.insert(ns.clone()) {
                continue;
            }
            let other_ns = match (&ns, rebased_unit) {
                (NamespaceId::Module(source), Some(new)) if source == unit => {
                    NamespaceId::Module(new.clone())
                }
                (NamespaceId::Associated(source), Some(new)) if &source.unit == unit => {
                    NamespaceId::Associated(SourceDeclRef {
                        unit: new.clone(),
                        item: source.item,
                    })
                }
                _ => ns.clone(),
            };
            let table = self.namespaces.get(&ns);
            let other_table = other.namespaces.get(&other_ns);
            let matches = match (table, other_table, rebased_unit) {
                (Some(a), Some(b), Some(new)) => {
                    a.owner == b.owner
                        && a.glob_allowed == b.glob_allowed
                        && same_name_tables(&a.names, &b.names, Some(unit), Some(new), false)
                }
                _ => table == other_table,
            };
            if !matches {
                return Ok(false);
            }
            if let Some(table) = table {
                for name in table.names.entries.keys() {
                    for namespace in NameNamespace::ALL {
                        if let Some(hit) = self
                            .lookup_member(&ctx, &ns, name.as_str(), namespace, cancel)?
                            .hit()
                        {
                            pending.push(hit.target);
                        }
                    }
                }
            }
        }
        Ok(self.package_aliases == other.package_aliases)
    }
}

impl NamespaceCatalog {
    /// Syntactic facade/module edges survive even when a terminal name is missing.
    pub(crate) fn path_dependencies(
        &self,
        path: &str,
        dependencies: &mut BTreeSet<ModuleIdentity>,
    ) {
        let mut segments = path.split("::");
        let Some(package) = segments.next() else {
            return;
        };
        let package = self
            .package_aliases
            .get(package)
            .filter(|packages| packages.len() == 1)
            .and_then(|packages| packages.first())
            .cloned()
            .unwrap_or_else(|| PackageId(package.into()));
        let components = segments.map(str::to_owned).collect::<Vec<_>>();
        for identity in self.modules.keys() {
            if identity.package == package && components.starts_with(&identity.path) {
                dependencies.insert(identity.clone());
            }
        }
    }
}
