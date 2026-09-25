//! Import facts are resolved once from immutable lowered sources and host declarations.
use crate::{
    builtin::surface,
    hir::ExportItem,
    host::{HostDeclarations, HostFunctionId, HostModuleId, HostTypeId},
    lower::LoweredModule,
};
use kagari_common::{
    Diagnostic, DiagnosticKind, Span,
    cancellation::{CancellationToken, Cancelled},
    identity::{FileId, ModuleIdentity, Revision},
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

mod bindings;
mod functions;
mod types;
pub(crate) use types::TypeCatalog;
pub use types::{ImportedType, ImportedTypes, SourceTypeId};
#[cfg(test)]
mod aggregate_tests;
#[cfg(test)]
mod signature_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod type_tests;
pub(crate) use functions::FunctionCatalog;
pub use functions::{ImportedFunction, ImportedFunctions, SourceFunctionId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceImport {
    pub module: ModuleIdentity,
    pub file: FileId,
    pub revision: Revision,
    /// Arena item identity is qualified by its source file and revision.
    pub item: Option<ExportItem>,
    pub members: Arc<BTreeMap<String, Vec<ExportItem>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportTarget {
    StandardModule(surface::StandardModule),
    StandardFunction(surface::StandardIntrinsic),
    HostModule(HostModuleId),
    HostFunction(HostFunctionId),
    HostType(HostTypeId),
    Source(SourceImport),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImport {
    pub alias: String,
    pub span: Span,
    pub target: Option<ImportTarget>,
    pub glob_root: bool,
    pub implicit_module: Option<crate::hir::ModuleId>,
    pub public_glob: bool,
    pub internal_namespace: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleImports {
    pub entries: Vec<ResolvedImport>,
    pub diagnostics: Vec<Diagnostic>,
    /// Final export targets, keyed by their local import or namespace member.
    /// Entries retain the direct source edge for initialization and navigation.
    bindings: HashMap<crate::resolver::ResolvedName, ImportTarget>,
    pub(crate) module_aliases: HashMap<crate::hir::ModuleId, usize>,
    namespace_entries: HashMap<ModuleIdentity, usize>,
}

impl ModuleImports {
    pub(crate) fn same_bindings(&self, other: &Self) -> bool {
        self.diagnostics.is_empty()
            && other.diagnostics.is_empty()
            && self.bindings.len() == other.bindings.len()
            && self
                .bindings
                .keys()
                .all(|key| self.resolved_name(*key) == other.resolved_name(*key))
            && self.entries.len() == other.entries.len()
            && self.entries.iter().zip(&other.entries).all(|(a, b)| {
                a.alias == b.alias
                    && a.target == b.target
                    && a.glob_root == b.glob_root
                    && a.implicit_module == b.implicit_module
                    && a.public_glob == b.public_glob
                    && a.internal_namespace == b.internal_namespace
            })
    }
}

#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub file: FileId,
    pub revision: Revision,
    pub imports: Arc<ModuleImports>,
    dependencies: Vec<ModuleIdentity>,
}

impl ModuleNode {
    pub fn dependencies(&self) -> &[ModuleIdentity] {
        &self.dependencies
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModuleGraph {
    nodes: BTreeMap<ModuleIdentity, ModuleNode>,
}

impl ModuleGraph {
    pub(crate) fn build<'a>(
        sources: impl IntoIterator<Item = &'a LoweredModule>,
        hosts: &HostDeclarations,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let mut modules = BTreeMap::new();
        let mut duplicate_identities = BTreeSet::new();
        for module in sources {
            cancel.check()?;
            let identity = module.source.module_identity().clone();
            match modules.entry(identity.clone()) {
                std::collections::btree_map::Entry::Occupied(_) => {
                    duplicate_identities.insert(identity);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(module);
                }
            }
        }
        let mut catalog = SourceCatalog::new(modules.values().copied(), None, cancel)?;
        let mut resolved = BTreeMap::new();
        // Public globs can expose members of another facade. Resolve their
        // exported names to a fixed point before building executable bindings.
        for _ in 0..=modules.len() * 2 + 1 {
            resolved.clear();
            for (identity, module) in &modules {
                cancel.check()?;
                resolved.insert(
                    identity.clone(),
                    resolve_imports(module, &catalog, hosts, cancel)?,
                );
            }
            let next = SourceCatalog::new(modules.values().copied(), Some(&resolved), cancel)?;
            if next.same_members(&catalog) {
                break;
            }
            catalog = next;
        }
        let mut nodes = BTreeMap::new();
        for (identity, module) in &modules {
            cancel.check()?;
            let imports = resolved.remove(identity).expect("resolved module");
            let dependencies = imports
                .entries
                .iter()
                .filter(|import| !import.internal_namespace)
                .filter_map(|import| match &import.target {
                    Some(ImportTarget::Source(target)) => Some(target.module.clone()),
                    _ => None,
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            nodes.insert(
                identity.clone(),
                ModuleNode {
                    file: module.source.id(),
                    revision: module.source.revision(),
                    imports: Arc::new(imports),
                    dependencies,
                },
            );
        }
        for identity in duplicate_identities {
            let node = nodes.get_mut(&identity).expect("duplicate module node");
            Arc::make_mut(&mut node.imports).diagnostics.push(
                Diagnostic::error(DiagnosticKind::DuplicateDeclaration {
                    name: identity.to_string(),
                })
                .with_span(Span::new(0, 0)),
            );
        }
        let mut graph = Self { nodes };
        graph.bind_exports(cancel)?;
        Ok(graph)
    }

    pub fn node(&self, module: &ModuleIdentity) -> Option<&ModuleNode> {
        self.nodes.get(module)
    }
    pub fn modules(&self) -> impl Iterator<Item = (&ModuleIdentity, &ModuleNode)> {
        self.nodes.iter()
    }

    /// Follow public re-exports, retaining the final source or offline host target.
    pub fn resolve_export(
        &self,
        mut target: SourceImport,
        cancel: &CancellationToken,
    ) -> Result<Option<ImportTarget>, Cancelled> {
        let mut visited = HashSet::new();
        loop {
            cancel.check()?;
            let Some(node) = self.node(&target.module) else {
                return Ok(None);
            };
            if node.file != target.file || node.revision != target.revision {
                return Ok(None);
            }
            let Some(ExportItem::Import(index)) = target.item else {
                if let Some(ExportItem::Module(id)) = target.item {
                    let Some(index) = node.imports.module_aliases.get(&id) else {
                        return Ok(None);
                    };
                    let Some(next) = node
                        .imports
                        .entries
                        .get(*index)
                        .and_then(|entry| entry.target.as_ref())
                    else {
                        return Ok(None);
                    };
                    match next {
                        ImportTarget::Source(next) => {
                            target = next.clone();
                            continue;
                        }
                        other => return Ok(Some(other.clone())),
                    }
                }
                return Ok(Some(ImportTarget::Source(target)));
            };
            if !visited.insert((target.file, index)) {
                return Ok(None);
            }
            let Some(next) = node
                .imports
                .entries
                .get(index)
                .and_then(|import| import.target.as_ref())
            else {
                return Ok(None);
            };
            match next {
                ImportTarget::Source(next) => target = next.clone(),
                target => return Ok(Some(target.clone())),
            }
        }
    }

    /// Stable order of the root's reachable source modules. Name references may be cyclic.
    pub fn reachable_order(
        &self,
        root: &ModuleIdentity,
        cancel: &CancellationToken,
    ) -> Result<Vec<ModuleIdentity>, ModuleOrderError> {
        if !self.nodes.contains_key(root) {
            return Err(ModuleOrderError::Missing(root.clone()));
        }
        let mut reachable = BTreeSet::new();
        let mut pending = vec![root.clone()];
        while let Some(module) = pending.pop() {
            cancel.check().map_err(|_| ModuleOrderError::Cancelled)?;
            if !reachable.insert(module.clone()) {
                continue;
            }
            let node = &self.nodes[&module];
            if !node.imports.diagnostics.is_empty() {
                return Err(ModuleOrderError::InvalidImports(module));
            }
            pending.extend(node.dependencies.iter().cloned());
        }
        Ok(reachable.into_iter().collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleOrderError {
    Missing(ModuleIdentity),
    InvalidImports(ModuleIdentity),
    Cancelled,
}

fn resolve_imports(
    module: &LoweredModule,
    catalog: &SourceCatalog<'_>,
    hosts: &HostDeclarations,
    cancel: &CancellationToken,
) -> Result<ModuleImports, Cancelled> {
    let mut result = ModuleImports::default();
    let local_items = module
        .module
        .functions
        .iter()
        .map(|item| item.name.as_str())
        .chain(module.module.consts.iter().map(|item| item.name.as_str()))
        .chain(module.module.modules.iter().map(|item| item.name.as_str()))
        .chain(module.module.structs.iter().map(|item| item.name.as_str()))
        .chain(module.module.enums.iter().map(|item| item.name.as_str()))
        .chain(module.module.traits.iter().map(|item| item.name.as_str()))
        .collect::<HashSet<_>>();
    let mut aliases = HashSet::new();
    let mut ambiguous = HashSet::new();
    for import in &module.module.imports {
        cancel.check()?;
        let path = normalize_import_path(&import.path, module.source.module_identity());
        if import.glob {
            let target = match resolve_path(&path, module.source.module_identity(), catalog, hosts)
            {
                Ok(target) => Some(target),
                Err(kind) => {
                    result
                        .diagnostics
                        .push(Diagnostic::error(kind).with_span(import.span));
                    None
                }
            };
            result.entries.push(ResolvedImport {
                alias: import.alias.clone(),
                span: import.span,
                target,
                glob_root: true,
                implicit_module: None,
                public_glob: false,
                internal_namespace: false,
            });
            continue;
        }
        let target =
            if local_items.contains(import.alias.as_str()) || !aliases.insert(&import.alias) {
                ambiguous.insert(import.alias.as_str());
                result.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::DuplicateImport {
                        name: import.alias.clone(),
                    })
                    .with_span(import.span),
                );
                None
            } else {
                match resolve_path(&path, module.source.module_identity(), catalog, hosts) {
                    Ok(target) => Some(target),
                    Err(kind) => {
                        result
                            .diagnostics
                            .push(Diagnostic::error(kind).with_span(import.span));
                        None
                    }
                }
            };
        result.entries.push(ResolvedImport {
            alias: import.alias.clone(),
            span: import.span,
            target,
            glob_root: false,
            implicit_module: None,
            public_glob: false,
            internal_namespace: false,
        });
    }
    for declaration in &module.module.modules {
        cancel.check()?;
        let mut child = module.source.module_identity().clone();
        child.path.push(declaration.name.clone());
        let path = child.to_string();
        let span = module.source_map.module_span(declaration.id);
        let target = match resolve_path(&path, module.source.module_identity(), catalog, hosts) {
            Ok(ImportTarget::Source(source)) if source.item.is_none() => {
                Some(ImportTarget::Source(source))
            }
            Ok(_) | Err(_) => {
                result.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::UnknownName { name: path }).with_span(span),
                );
                None
            }
        };
        let index = result.entries.len();
        result.module_aliases.insert(declaration.id, index);
        result.entries.push(ResolvedImport {
            alias: declaration.name.clone(),
            span,
            target,
            glob_root: false,
            implicit_module: Some(declaration.id),
            public_glob: false,
            internal_namespace: false,
        });
    }
    let mut glob_names = HashMap::<String, usize>::new();
    for (root, import) in module.module.imports.iter().enumerate() {
        cancel.check()?;
        if !import.glob {
            continue;
        }
        let members = match result.entries[root].target.as_ref() {
            Some(ImportTarget::Source(source)) if source.item.is_none() => source
                .members
                .iter()
                .filter_map(|(name, items)| {
                    let [item] = items.as_slice() else {
                        return None;
                    };
                    let mut target = source.clone();
                    target.item = Some(*item);
                    Some((name.clone(), ImportTarget::Source(target)))
                })
                .collect::<Vec<_>>(),
            Some(ImportTarget::StandardModule(module)) => {
                surface::standard_functions_in_module(*module)
                    .map(|function| {
                        (
                            function.name.to_owned(),
                            ImportTarget::StandardFunction(function.intrinsic),
                        )
                    })
                    .collect()
            }
            Some(ImportTarget::HostModule(module)) => hosts
                .members_of_module(*module)
                .into_iter()
                .map(|(name, resolved)| {
                    (
                        name,
                        match resolved {
                            crate::resolver::ResolvedName::HostFunction(id) => {
                                ImportTarget::HostFunction(id)
                            }
                            crate::resolver::ResolvedName::HostType(id) => {
                                ImportTarget::HostType(id)
                            }
                            crate::resolver::ResolvedName::HostModule(id) => {
                                ImportTarget::HostModule(id)
                            }
                            _ => unreachable!("host module member"),
                        },
                    )
                })
                .collect(),
            Some(_) => {
                result.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidGlobTarget {
                        path: import.path.clone(),
                    })
                    .with_span(import.span),
                );
                Vec::new()
            }
            None => Vec::new(),
        };
        for (name, target) in members {
            cancel.check()?;
            if local_items.contains(name.as_str()) || aliases.contains(&name) {
                continue;
            }
            if let Some(previous) = glob_names.get(&name).copied() {
                if result.entries[previous].target.as_ref() != Some(&target) {
                    result.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::DuplicateImport { name: name.clone() })
                            .with_span(import.span),
                    );
                    result.entries[previous].target = None;
                }
                continue;
            }
            glob_names.insert(name.clone(), result.entries.len());
            result.entries.push(ResolvedImport {
                alias: name,
                span: import.span,
                target: Some(target),
                glob_root: false,
                implicit_module: None,
                public_glob: import.visibility == crate::hir::Visibility::Public,
                internal_namespace: false,
            });
        }
    }
    let roots = result
        .entries
        .iter()
        .filter_map(|entry| match &entry.target {
            Some(ImportTarget::Source(source)) if source.item.is_none() => {
                Some(source.module.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for entries in catalog.paths.values() {
        for entry in entries {
            cancel.check()?;
            let identity = entry.source.module_identity();
            if roots.iter().any(|root| {
                root.package == identity.package
                    && identity.path.starts_with(&root.path)
                    && identity.path.len() > root.path.len()
            }) && !result.namespace_entries.contains_key(identity)
            {
                result
                    .namespace_entries
                    .insert(identity.clone(), result.entries.len());
                result.entries.push(ResolvedImport {
                    alias: String::new(),
                    span: Span::new(0, 0),
                    target: Some(ImportTarget::Source(entry.target(None))),
                    glob_root: false,
                    implicit_module: None,
                    public_glob: false,
                    internal_namespace: true,
                });
            }
        }
    }
    for entry in &mut result.entries {
        cancel.check()?;
        if ambiguous.contains(entry.alias.as_str()) {
            entry.target = None;
        }
    }
    Ok(result)
}

fn resolve_path(
    path: &str,
    importer: &ModuleIdentity,
    catalog: &SourceCatalog<'_>,
    hosts: &HostDeclarations,
) -> Result<ImportTarget, DiagnosticKind> {
    let mut candidates = Vec::new();
    if let Some(module) = surface::standard_module(path) {
        candidates.push(ImportTarget::StandardModule(module.kind));
    }
    if let Some(function) = path.rsplit_once("::").and_then(|(module, name)| {
        surface::standard_module(module)
            .and_then(|module| surface::standard_function(module.kind, name))
    }) {
        candidates.push(ImportTarget::StandardFunction(function.intrinsic));
    }
    if let Some(function) = hosts.resolve(path) {
        candidates.push(ImportTarget::HostFunction(function));
    }
    if let Some(ty) = hosts.resolve_type(path) {
        candidates.push(ImportTarget::HostType(ty));
    }
    if let Some(module) = hosts.module(path) {
        candidates.push(ImportTarget::HostModule(module));
    }
    let mut private = false;
    for module in catalog.paths.get(path).into_iter().flatten() {
        if source_module_accessible(module.source.module_identity(), importer, catalog) {
            candidates.push(ImportTarget::Source(module.target(None)));
        } else {
            private = true;
        }
    }
    if let Some((parent, member)) = path.rsplit_once("::") {
        for module in catalog.paths.get(parent).into_iter().flatten() {
            if !source_module_accessible(module.source.module_identity(), importer, catalog) {
                private = true;
                continue;
            }
            match module.members.get(member).map(Vec::as_slice) {
                Some([item])
                    if matches!(item, ExportItem::Module(_))
                        && catalog.paths.contains_key(path) => {}
                Some([item]) => {
                    let namespace = match item {
                        ExportItem::Import(index) => module
                            .reexports
                            .get(index)
                            .cloned()
                            .and_then(|target| canonical_namespace_target(target, catalog)),
                        _ => None,
                    };
                    candidates.push(
                        namespace
                            .unwrap_or_else(|| ImportTarget::Source(module.target(Some(*item)))),
                    );
                }
                Some(_) => return Err(DiagnosticKind::AmbiguousImport { path: path.into() }),
                None => private = true,
            }
        }
    }
    match candidates.len() {
        1 => Ok(candidates.pop().unwrap()),
        0 if private => Err(DiagnosticKind::ImportNotPublic { path: path.into() }),
        0 => Err(DiagnosticKind::UnknownName { name: path.into() }),
        _ => Err(DiagnosticKind::AmbiguousImport { path: path.into() }),
    }
}

fn canonical_namespace_target(
    mut target: ImportTarget,
    catalog: &SourceCatalog<'_>,
) -> Option<ImportTarget> {
    let mut seen = HashSet::new();
    loop {
        match &target {
            ImportTarget::Source(source) if source.item.is_none() => return Some(target),
            ImportTarget::Source(source) => {
                let Some(ExportItem::Import(index)) = source.item else {
                    return None;
                };
                if !seen.insert((source.file, index)) {
                    return None;
                }
                let entry =
                    catalog
                        .paths
                        .get(&source.module.to_string())?
                        .iter()
                        .find(|entry| {
                            entry.source.id() == source.file
                                && entry.source.revision() == source.revision
                        })?;
                target = entry.reexports.get(&index)?.clone();
            }
            ImportTarget::StandardModule(_) | ImportTarget::HostModule(_) => return Some(target),
            _ => return None,
        }
    }
}

fn source_module_accessible(
    target: &ModuleIdentity,
    importer: &ModuleIdentity,
    catalog: &SourceCatalog<'_>,
) -> bool {
    let mut child = target.clone();
    while child.path.len() > 1 {
        let name = child.path.pop().expect("nonempty child path");
        if importer.package == child.package && importer.path.starts_with(&child.path) {
            return true;
        }
        let Some(parent) = catalog.paths.get(&child.to_string()) else {
            break;
        };
        if !parent
            .iter()
            .any(|module| module.members.contains_key(&name))
        {
            return false;
        }
    }
    true
}

struct SourceCatalog<'a> {
    paths: BTreeMap<String, Vec<SourceCatalogEntry<'a>>>,
}

struct SourceCatalogEntry<'a> {
    source: &'a kagari_common::SourceFile,
    members: Arc<BTreeMap<String, Vec<ExportItem>>>,
    reexports: BTreeMap<usize, ImportTarget>,
}

impl<'a> SourceCatalog<'a> {
    fn new(
        sources: impl IntoIterator<Item = &'a LoweredModule>,
        imports: Option<&BTreeMap<ModuleIdentity, ModuleImports>>,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let mut paths = BTreeMap::<_, Vec<_>>::new();
        for module in sources {
            cancel.check()?;
            let mut members = BTreeMap::<_, Vec<_>>::new();
            for export in &module.module.exports {
                cancel.check()?;
                members
                    .entry(export.name.clone())
                    .or_default()
                    .push(export.item);
            }
            let resolved_imports = imports.and_then(|all| all.get(module.source.module_identity()));
            if let Some(imports) = resolved_imports {
                for (index, import) in imports.entries.iter().enumerate() {
                    cancel.check()?;
                    if import.public_glob
                        && import.target.is_some()
                        && !members.contains_key(&import.alias)
                    {
                        members.insert(import.alias.clone(), vec![ExportItem::Import(index)]);
                    }
                }
            }
            paths
                .entry(module.source.module_identity().to_string())
                .or_default()
                .push(SourceCatalogEntry {
                    source: &module.source,
                    members: Arc::new(members),
                    reexports: resolved_imports.map_or_else(BTreeMap::new, |imports| {
                        imports
                            .entries
                            .iter()
                            .enumerate()
                            .filter_map(|(index, entry)| {
                                entry.target.clone().map(|target| (index, target))
                            })
                            .collect()
                    }),
                });
        }
        Ok(Self { paths })
    }

    fn same_members(&self, other: &Self) -> bool {
        self.paths.len() == other.paths.len()
            && self.paths.iter().all(|(path, entries)| {
                other.paths.get(path).is_some_and(|old| {
                    entries.len() == old.len()
                        && entries
                            .iter()
                            .zip(old)
                            .all(|(a, b)| a.members == b.members && a.reexports == b.reexports)
                })
            })
    }
}

fn normalize_import_path(path: &str, current: &ModuleIdentity) -> String {
    let mut segments = path.split("::").peekable();
    let mut base = current.path.clone();
    match segments.peek().copied() {
        Some("self") => {
            segments.next();
        }
        Some("crate") => {
            segments.next();
            base.truncate(1);
        }
        Some("super") => {
            while segments.peek() == Some(&"super") {
                segments.next();
                if base.len() <= 1 {
                    return path.to_owned();
                }
                base.pop();
            }
        }
        _ => return path.to_owned(),
    }
    base.extend(segments.map(str::to_owned));
    format!("{}::{}", current.package.0, base.join("::"))
}

impl SourceCatalogEntry<'_> {
    fn target(&self, item: Option<ExportItem>) -> SourceImport {
        SourceImport {
            module: self.source.module_identity().clone(),
            file: self.source.id(),
            revision: self.source.revision(),
            item,
            members: self.members.clone(),
        }
    }
}
