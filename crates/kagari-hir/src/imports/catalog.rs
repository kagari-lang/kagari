//! Snapshot-owned, unfiltered namespaces. Targets never carry member tables.
use crate::{
    imports::{
        BindingCandidate, BindingOrigin, DirectiveId, NamespaceId, ResolvedTarget, SourceDeclRef,
        SourceItem, SourceUnit, bindings::LookupOutcome, cache::same_name_tables,
        solver::Observation,
    },
    native::paths::source_package_alias,
    resolver::table::NameTable,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{
        DefinitionKind, DefinitionPath, ModuleIdentity, PackageId,
        map::DefinitionContext,
        table::{DefinitionTable, module::ModuleHandle},
    },
};
use kagari_source::package::PackageEnvironment;
use kagari_types::{declaration::names::NameNamespace, visibility::Visibility};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

/// Unfiltered member candidates for one namespace and its glob-expansion policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceTable {
    /// Logical owner used when constructing/accessing this namespace.
    pub owner: ModuleHandle,
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
#[derive(Debug, Clone)]
pub struct NamespaceCatalog {
    /// Immutable identity prefix owning every module handle in this catalog.
    pub definitions: DefinitionTable,
    /// All exact source units grouped by logical module identity; multiple entries remain ambiguous.
    pub(crate) modules: BTreeMap<ModuleHandle, Vec<SourceUnit>>,
    /// Namespace identity to owner, shared name table and glob policy.
    pub(crate) namespaces: HashMap<NamespaceId, NamespaceTable>,
    /// Installed package aliases; multiple package targets make an alias ambiguous.
    pub(crate) installed_aliases: BTreeMap<String, BTreeSet<PackageId>>,
    /// Canonical dependency modules retained by checked native declarations.
    /// These resolve only for that native importer, never as global source aliases.
    pub(crate) native_dependencies: BTreeMap<ModuleHandle, BTreeSet<ModuleHandle>>,
    /// Trusted declaration references admit only their canonical trait target.
    /// Ordinary source importers receive no additional visibility.
    pub(crate) native_trait_references: BTreeMap<ModuleHandle, BTreeSet<DefinitionPath>>,
    pub(crate) packages: PackageEnvironment,
}

/// A canonical target plus the selected binding origins along its lookup path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupHit {
    pub(crate) support: HashSet<(DirectiveId, NameNamespace)>,
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
    pub importer: &'a ModuleHandle,
    pub(crate) avoid: Option<(DirectiveId, NameNamespace)>,
    pub(crate) observations: Option<&'a RefCell<HashSet<Observation>>>,
}

impl<'a> LookupContext<'a> {
    pub fn new(importer: &'a ModuleHandle) -> Self {
        Self {
            importer,
            observations: None,
            avoid: None,
        }
    }

    pub(crate) fn observe(&self, namespace: &NamespaceId) {
        if let Some(observations) = self.observations {
            observations
                .borrow_mut()
                .insert(Observation::Namespace(namespace.clone()));
        }
    }
}

impl NamespaceCatalog {
    pub(crate) fn new(definitions: DefinitionTable, packages: PackageEnvironment) -> Self {
        Self {
            definitions,
            packages,
            modules: BTreeMap::new(),
            namespaces: HashMap::new(),
            installed_aliases: BTreeMap::new(),
            native_dependencies: BTreeMap::new(),
            native_trait_references: BTreeMap::new(),
        }
    }

    /// Borrow an identity already admitted into this catalog's scope.
    pub(crate) fn identity(&self, module: ModuleHandle) -> &ModuleIdentity {
        self.definitions
            .module_identity(module)
            .expect("catalog module scope")
    }

    pub(crate) fn allows(
        &self,
        visibility: Visibility,
        owner: ModuleHandle,
        accessor: ModuleHandle,
    ) -> bool {
        let (Ok(owner), Ok(accessor)) = (
            self.definitions.module_identity(owner),
            self.definitions.module_identity(accessor),
        ) else {
            return false;
        };
        visibility.allows(owner, accessor)
    }

    pub(crate) fn valid(&self, unit: &SourceUnit) -> bool {
        self.modules
            .get(&unit.module)
            .is_some_and(|units| units.contains(unit))
    }

    /// Selects one member, checking source validity, precedence and importer visibility.
    ///
    /// Lookup failures are returned as
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
        if self.definitions.module_identity(*ctx.importer).is_err() {
            return Ok(LookupResult::StaleSource);
        }
        if let Some(observations) = ctx.observations {
            observations
                .borrow_mut()
                .insert(Observation::Namespace(ns.clone()));
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
        let selected = table.names.candidates(name, namespace);
        if selected.is_none_or(|(_, strong)| !strong)
            && table
                .names
                .pending_globs
                .iter()
                .any(|candidate| self.allows(candidate.visibility, candidate.owner, *ctx.importer))
        {
            return Ok(LookupResult::Unresolved);
        }
        let Some((candidates, strong)) = selected else {
            return Ok(LookupResult::Missing);
        };
        if strong
            && candidates.len() > 1
            && candidates
                .iter()
                .any(|candidate| candidate.resolution == LookupOutcome::Pending)
        {
            return Ok(LookupResult::Unresolved);
        }
        if strong && candidates.len() > 1 {
            return Ok(LookupResult::Ambiguous(candidates.to_vec()));
        }
        let admitted = candidates
            .iter()
            .filter(|c| {
                self.allows(c.visibility, c.owner, *ctx.importer)
                    || self.referenced_trait(ctx, ns, name, c)
            })
            .cloned()
            .collect::<Vec<_>>();
        if admitted.is_empty() {
            return Ok(LookupResult::Inaccessible(candidates.to_vec()));
        }
        let acyclic = admitted
            .iter()
            .filter(|candidate| {
                ctx.avoid
                    .as_ref()
                    .is_none_or(|key| !candidate.support.contains(key))
            })
            .cloned()
            .collect::<Vec<_>>();
        if acyclic.is_empty() {
            return Ok(LookupResult::Unresolved);
        }
        Ok(NameTable::select(&acyclic, strong))
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
        ctx: &LookupContext<'_>,
        target: &ResolvedTarget,
        cancel: &CancellationToken,
    ) -> Result<NamespaceResult, Cancelled> {
        cancel.check()?;
        if self.definitions.module_identity(*ctx.importer).is_err() {
            return Ok(NamespaceResult::StaleSource);
        }
        Ok(match target {
            ResolvedTarget::Namespace(ns) => match ns {
                NamespaceId::InstalledPrefix(module)
                    if self.definitions.module_identity(*module).is_err() =>
                {
                    NamespaceResult::StaleSource
                }
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
        })
    }

    /// Walks borrowed suffix components from an existing root lookup.
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
        suffix: &[String],
        namespace: NameNamespace,
        cancel: &CancellationToken,
    ) -> Result<LookupResult, Cancelled> {
        let mut components = suffix.iter().filter(|c| !c.is_empty()).peekable();
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
                hit.support.extend(previous.support);
            }
        }
        Ok(root)
    }

    pub(crate) fn absolute(
        &self,
        ctx: &LookupContext<'_>,
        path: &[String],
        namespace: NameNamespace,
        cancel: &CancellationToken,
    ) -> Result<LookupResult, Cancelled> {
        cancel.check()?;
        if let Some(observations) = ctx.observations {
            observations.borrow_mut().insert(Observation::InputUniverse);
        }
        let Some((packages, components)) = self.package_path(ctx.importer, path) else {
            return Ok(LookupResult::Missing);
        };
        if packages.len() > 1 {
            return Ok(LookupResult::Ambiguous(vec![]));
        }
        let Some(package) = packages.first() else {
            return Ok(LookupResult::Missing);
        };
        for end in 0..=components.len() {
            let Some(identity) = self
                .definitions
                .lookup_module_path(package, &components[..end])
            else {
                continue;
            };
            if let Some(units) = self.modules.get(&identity) {
                if end == components.len() && namespace == NameNamespace::Value {
                    return Ok(LookupResult::Missing);
                }
                if units.len() != 1 {
                    return Ok(LookupResult::Ambiguous(vec![]));
                }
                if !self.module_accessible(ctx, &identity, cancel)? {
                    return Ok(LookupResult::Inaccessible(vec![]));
                }
                let root = LookupResult::Found(LookupHit {
                    support: HashSet::new(),
                    namespace: NameNamespace::Type,
                    target: ResolvedTarget::Namespace(NamespaceId::Module(units[0].clone())),
                    via: vec![],
                });
                let result = self.resolve_path(ctx, root, &components[end..], namespace, cancel)?;
                let terminal = self.definitions.lookup_module_path(package, &components);
                let physical_collision = namespace == NameNamespace::Type && terminal.and_then(|id| self.modules.get(&id)).is_some_and(|units| {
                    matches!(&result, LookupResult::Found(hit) if !matches!(&hit.target, ResolvedTarget::Namespace(NamespaceId::Module(unit)) if units.as_slice() == [unit.clone()]))
                });
                return Ok(if physical_collision {
                    LookupResult::Ambiguous(vec![])
                } else {
                    result
                });
            }
        }
        let Some(root) = self.definitions.lookup_module_path(package, &[]) else {
            return Ok(LookupResult::Missing);
        };
        let prefix = NamespaceId::InstalledPrefix(root);
        if self.namespaces.contains_key(&prefix) {
            return self.resolve_path(
                ctx,
                LookupResult::Found(LookupHit {
                    support: HashSet::new(),
                    namespace: NameNamespace::Type,
                    target: ResolvedTarget::Namespace(prefix),
                    via: vec![],
                }),
                &components,
                namespace,
                cancel,
            );
        }
        Ok(LookupResult::Missing)
    }

    fn module_accessible(
        &self,
        ctx: &LookupContext<'_>,
        identity: &ModuleHandle,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        let mut current = *identity;
        while let Some(parent) = self
            .definitions
            .module_parent(current)
            .expect("catalog root")
        {
            cancel.check()?;
            let name = self.identity(current).path.last().expect("child");
            current = parent;
            if let Some(units) = self.modules.get(&parent) {
                for unit in units {
                    if matches!(
                        self.lookup_member(
                            ctx,
                            &NamespaceId::Module(unit.clone()),
                            name,
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
        cancel: &CancellationToken,
    ) -> Result<HashSet<SourceDeclRef>, Cancelled> {
        let Some(unit) = names.unit.as_ref() else {
            return Ok(HashSet::new());
        };
        let ctx = LookupContext {
            importer: &unit.module,
            observations: None,
            avoid: None,
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
        self.add_package_namespaces(&ctx, &mut pending, cancel)?;
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
        name: &str,
        namespace: NameNamespace,
        cancel: &CancellationToken,
    ) -> Option<LookupHit> {
        let unit = names.unit.as_ref()?;
        let ctx = LookupContext {
            importer: &unit.module,
            observations: None,
            avoid: None,
        };
        let (root, suffix) = name.split_once("::").unwrap_or((name, ""));
        // Root lookup stays literal; its category uses raw suffix emptiness even
        // when parsing below discards every suffix component.
        let result = names.hit(
            root,
            if suffix.is_empty() {
                namespace
            } else {
                NameNamespace::Type
            },
        );
        let result = if matches!(result, LookupResult::Missing) {
            let path = name
                .split("::")
                .filter(|component| !component.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            self.absolute(&ctx, &path, namespace, cancel).ok()?
        } else {
            let suffix = suffix
                .split("::")
                .filter(|component| !component.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            self.resolve_path(&ctx, result, &suffix, namespace, cancel)
                .ok()?
        };
        result.hit()
    }
}

impl NamespaceCatalog {
    /// Qualified paths need signature projections even without a `use` binding.
    fn add_package_namespaces(
        &self,
        ctx: &LookupContext<'_>,
        pending: &mut Vec<ResolvedTarget>,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        let dependencies = self
            .packages
            .scope(&self.identity(*ctx.importer).package)
            .map(|scope| &scope.dependencies);
        for (identity, units) in &self.modules {
            cancel.check()?;
            let portable = self.identity(*identity);
            let own = portable.package == self.identity(*ctx.importer).package;
            let direct = dependencies.is_some_and(|aliases| {
                aliases.values().any(|package| package == &portable.package)
            });
            let native = self
                .native_dependencies
                .get(ctx.importer)
                .is_some_and(|dependencies| dependencies.contains(identity));
            if (own || native || (direct && portable.path.is_empty()))
                && units.len() == 1
                && self.module_accessible(ctx, identity, cancel)?
            {
                pending.push(ResolvedTarget::Namespace(NamespaceId::Module(
                    units[0].clone(),
                )));
            }
        }
        Ok(())
    }
}

impl NamespaceCatalog {
    /// A retained file may keep its catalog only when every namespace it can
    /// enter still has the same qualified targets, candidates and access facts.
    pub(crate) fn same_namespaces(
        &self,
        other: &Self,
        names: &NameTable,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        self.same_namespace_inputs(other, names, None, cancel, false)
    }

    /// Reuse checked facts across a local lowering only after comparing all
    /// reachable member bindings. The existing remappers handle local arena IDs;
    /// foreign namespace identities and targets must still match exactly.
    pub(crate) fn same_reuse_namespaces(
        &self,
        other: &Self,
        names: &NameTable,
        other_names: &NameTable,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        let (Some(unit), Some(other_unit)) = (&names.unit, &other_names.unit) else {
            return Ok(false);
        };
        if unit.module != other_unit.module || unit.file != other_unit.file {
            return Ok(false);
        }
        self.same_namespace_inputs(other, names, Some(other_unit), cancel, false)
    }

    /// Compare signature namespace bindings independently of foreign function body revisions.
    /// The caller must also compare imported type surfaces before reusing typed signatures.
    pub(crate) fn same_signature_namespaces(
        &self,
        other: &Self,
        names: &NameTable,
        other_names: &NameTable,
        cancel: &CancellationToken,
    ) -> Result<bool, Cancelled> {
        let (Some(unit), Some(other_unit)) = (&names.unit, &other_names.unit) else {
            return Ok(false);
        };
        if unit.module != other_unit.module || unit.file != other_unit.file {
            return Ok(false);
        }
        self.same_namespace_inputs(other, names, Some(other_unit), cancel, true)
    }

    fn same_namespace_inputs(
        &self,
        other: &Self,
        names: &NameTable,
        rebased_unit: Option<&SourceUnit>,
        cancel: &CancellationToken,
        signature: bool,
    ) -> Result<bool, Cancelled> {
        let Some(unit) = names.unit.as_ref() else {
            return Ok(false);
        };
        let ctx = LookupContext {
            importer: &unit.module,
            observations: None,
            avoid: None,
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
        let package_start = pending.len();
        self.add_package_namespaces(&ctx, &mut pending, cancel)?;
        let mut other_packages = Vec::new();
        other.add_package_namespaces(&ctx, &mut other_packages, cancel)?;
        let package_namespaces = &pending[package_start..];
        if package_namespaces.len() != other_packages.len()
            || !package_namespaces
                .iter()
                .zip(&other_packages)
                .all(|(a, b)| {
                    matches!((a, b), (
                    ResolvedTarget::Namespace(NamespaceId::Module(a)),
                    ResolvedTarget::Namespace(NamespaceId::Module(b))
                ) if a.module == b.module && a.file == b.file)
                })
        {
            return Ok(false);
        }
        let mut seen = HashSet::new();
        while let Some(target) = pending.pop() {
            cancel.check()?;
            let NamespaceResult::Found(ns) = self.namespace_of(&ctx, &target, cancel)? else {
                continue;
            };
            if !seen.insert(ns.clone()) {
                continue;
            }
            let foreign_unit = match &ns {
                NamespaceId::Module(source) => Some(source),
                NamespaceId::Associated(source) => Some(&source.unit),
                _ => None,
            }
            .filter(|_| signature)
            .and_then(|source| {
                other
                    .modules
                    .get(&source.module)
                    .filter(|units| units.len() == 1)
                    .and_then(|units| units.iter().find(|candidate| candidate.file == source.file))
            });
            let mapped_unit = if signature {
                foreign_unit
            } else {
                rebased_unit
            };
            let other_ns = match (&ns, mapped_unit) {
                (NamespaceId::Module(source), Some(new)) if signature || source == unit => {
                    NamespaceId::Module(new.clone())
                }
                (NamespaceId::Associated(source), Some(new))
                    if signature || &source.unit == unit =>
                {
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
                        && same_name_tables(&a.names, &b.names, Some(unit), Some(new), signature)
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
        Ok(self.installed_aliases == other.installed_aliases
            && self.native_dependencies == other.native_dependencies
            && self.native_trait_references == other.native_trait_references
            && self.packages == other.packages)
    }
}

impl NamespaceCatalog {
    fn referenced_trait(
        &self,
        ctx: &LookupContext<'_>,
        namespace: &NamespaceId,
        name: &str,
        candidate: &BindingCandidate,
    ) -> bool {
        let (NamespaceId::Module(unit), BindingOrigin::Declaration(source)) =
            (namespace, &candidate.origin)
        else {
            return false;
        };
        let LookupOutcome::Resolved(ResolvedTarget::Source(target)) = &candidate.resolution else {
            return false;
        };
        if source.unit != *unit || !matches!(source.item, SourceItem::Trait(_)) || target != source
        {
            return false;
        }
        self.native_trait_references
            .get(ctx.importer)
            .is_some_and(|references| {
                references.iter().any(|reference| {
                    self.definitions.lookup_module(&reference.module) == Some(unit.module)
                        && reference.path.len() == 1
                        && reference.path[0].kind == DefinitionKind::Trait
                        && reference.path[0].occurrence == 0
                        && reference.path[0].name == name
                })
            })
    }

    fn package_path(
        &self,
        importer: &ModuleHandle,
        path: &[String],
    ) -> Option<(BTreeSet<PackageId>, Vec<String>)> {
        let importer_identity = self.identity(*importer);
        let mut segments = path
            .iter()
            .map(String::as_str)
            .filter(|segment| !segment.is_empty())
            .peekable();
        let first = segments.next()?;
        let (packages, mut components) = match first {
            "crate" => ([importer_identity.package.clone()].into(), vec![]),
            "self" => (
                [importer_identity.package.clone()].into(),
                importer_identity.path.clone(),
            ),
            "super" => {
                let mut base = importer_identity.path.clone();
                base.pop()?;
                while segments.peek() == Some(&"super") {
                    segments.next();
                    base.pop()?;
                }
                ([importer_identity.package.clone()].into(), base)
            }
            alias => (self.package_targets(importer, alias), vec![]),
        };
        components.extend(segments.map(str::to_owned));
        Some((packages, components))
    }

    fn package_targets(&self, importer: &ModuleHandle, name: &str) -> BTreeSet<PackageId> {
        let importer_identity = self.identity(*importer);
        if name == importer_identity.package.0 {
            return [importer_identity.package.clone()].into();
        }
        if let Some(dependencies) = self.native_dependencies.get(importer) {
            let targets = dependencies
                .iter()
                .filter(|dependency| {
                    self.identity(**dependency).package.0 == name
                        || source_package_alias(&self.identity(**dependency).package).as_deref()
                            == Some(name)
                })
                .map(|dependency| self.identity(*dependency).package.clone())
                .collect::<BTreeSet<_>>();
            if !targets.is_empty() {
                return targets;
            }
        }
        if let Some(target) = self
            .packages
            .scope(&importer_identity.package)
            .and_then(|scope| scope.dependencies.get(name))
        {
            return [target.clone()].into();
        }
        self.installed_aliases
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    /// Syntactic facade/module edges survive even when a terminal name is missing.
    pub(crate) fn path_dependencies(
        &self,
        importer: &ModuleHandle,
        path: &[String],
        dependencies: &mut BTreeSet<ModuleHandle>,
    ) {
        let Some((packages, components)) = self.package_path(importer, path) else {
            return;
        };
        if packages.len() != 1 {
            return;
        }
        let package = packages.first().expect("one package");
        for identity in self.modules.keys() {
            if &self.identity(*identity).package == package
                && components.starts_with(&self.identity(*identity).path)
            {
                dependencies.insert(*identity);
            }
        }
    }
}

impl Default for NamespaceCatalog {
    fn default() -> Self {
        Self::new(
            DefinitionContext::new()
                .expect("identity capacity")
                .snapshot(),
            PackageEnvironment::default(),
        )
    }
}
