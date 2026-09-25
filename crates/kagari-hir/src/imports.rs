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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleImports {
    pub entries: Vec<ResolvedImport>,
    pub diagnostics: Vec<Diagnostic>,
    /// Final export targets, keyed by their local import or namespace member.
    /// Entries retain the direct source edge for initialization and navigation.
    bindings: HashMap<crate::resolver::ResolvedName, ImportTarget>,
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
            && self
                .entries
                .iter()
                .zip(&other.entries)
                .all(|(a, b)| a.alias == b.alias && a.target == b.target)
    }
}

#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub file: FileId,
    pub revision: Revision,
    pub imports: Arc<ModuleImports>,
    dependencies: Vec<ModuleIdentity>,
    cycle: Option<Arc<[ModuleIdentity]>>,
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
        modules: impl IntoIterator<Item = &'a LoweredModule>,
        hosts: &HostDeclarations,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let modules = modules
            .into_iter()
            .map(|m| (m.source.module_identity().clone(), m))
            .collect::<BTreeMap<_, _>>();
        let catalog = SourceCatalog::new(modules.values().copied(), cancel)?;
        let mut nodes = BTreeMap::new();
        for (identity, module) in &modules {
            cancel.check()?;
            let imports = resolve_imports(module, &catalog, hosts, cancel)?;
            let dependencies = imports
                .entries
                .iter()
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
                    cycle: None,
                },
            );
        }
        let mut graph = Self { nodes };
        graph.mark_cycles(cancel)?;
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

    /// Dependency-first order of the root's reachable graph; unrelated cycles do not block it.
    pub fn initialization_order(
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
            if let Some(cycle) = &node.cycle {
                return Err(ModuleOrderError::Cycle(cycle.to_vec()));
            }
            if !node.imports.diagnostics.is_empty() {
                return Err(ModuleOrderError::InvalidImports(module));
            }
            pending.extend(node.dependencies.iter().cloned());
        }
        let mut remaining = BTreeMap::new();
        let mut dependents = BTreeMap::<_, Vec<_>>::new();
        let mut ready = BTreeSet::new();
        for module in &reachable {
            cancel.check().map_err(|_| ModuleOrderError::Cancelled)?;
            let dependencies = &self.nodes[module].dependencies;
            remaining.insert(module.clone(), dependencies.len());
            if dependencies.is_empty() {
                ready.insert(module.clone());
            }
            for dependency in dependencies {
                dependents
                    .entry(dependency.clone())
                    .or_default()
                    .push(module.clone());
            }
        }
        let mut order = Vec::with_capacity(reachable.len());
        while let Some(module) = ready.pop_first() {
            cancel.check().map_err(|_| ModuleOrderError::Cancelled)?;
            for dependent in dependents.get(&module).into_iter().flatten() {
                let count = remaining.get_mut(dependent).expect("reachable dependent");
                *count -= 1;
                if *count == 0 {
                    ready.insert(dependent.clone());
                }
            }
            order.push(module);
        }
        Ok(order)
    }

    fn mark_cycles(&mut self, cancel: &CancellationToken) -> Result<(), Cancelled> {
        // Iterative Kosaraju: source depth must not consume the Rust call stack.
        let ids = self.nodes.keys().cloned().collect::<Vec<_>>();
        let indices = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect::<HashMap<_, _>>();
        let edges = ids
            .iter()
            .map(|id| {
                self.nodes[id]
                    .dependencies
                    .iter()
                    .map(|id| indices[id])
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut reverse = vec![Vec::new(); ids.len()];
        for (from, dependencies) in edges.iter().enumerate() {
            for to in dependencies {
                reverse[*to].push(from);
            }
        }
        let mut visited = vec![false; ids.len()];
        let mut finish = Vec::new();
        for root in 0..ids.len() {
            cancel.check()?;
            if visited[root] {
                continue;
            }
            visited[root] = true;
            let mut stack = vec![(root, 0)];
            while let Some((node, next)) = stack.last_mut() {
                cancel.check()?;
                if let Some(child) = edges[*node].get(*next) {
                    *next += 1;
                    if !visited[*child] {
                        visited[*child] = true;
                        stack.push((*child, 0));
                    }
                } else {
                    finish.push(*node);
                    stack.pop();
                }
            }
        }
        visited.fill(false);
        for root in finish.into_iter().rev() {
            if visited[root] {
                continue;
            }
            let mut component = Vec::new();
            let mut stack = vec![root];
            visited[root] = true;
            while let Some(node) = stack.pop() {
                cancel.check()?;
                component.push(node);
                for parent in &reverse[node] {
                    if !visited[*parent] {
                        visited[*parent] = true;
                        stack.push(*parent);
                    }
                }
            }
            if component.len() == 1 && !edges[root].contains(&root) {
                continue;
            }
            component.sort_unstable();
            let cycle: Arc<[ModuleIdentity]> = component.iter().map(|i| ids[*i].clone()).collect();
            let cycle_names: Arc<[String]> = cycle.iter().map(ToString::to_string).collect();
            let members = cycle.iter().collect::<HashSet<_>>();
            for index in &component {
                let node = self.nodes.get_mut(&ids[*index]).unwrap();
                node.cycle = Some(cycle.clone());
                let imports = Arc::make_mut(&mut node.imports);
                for import in &imports.entries {
                    if let Some(ImportTarget::Source(target)) = &import.target
                        && members.contains(&target.module)
                    {
                        imports.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::CyclicImport {
                                modules: cycle_names.clone(),
                            })
                            .with_span(import.span),
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleOrderError {
    Missing(ModuleIdentity),
    InvalidImports(ModuleIdentity),
    Cycle(Vec<ModuleIdentity>),
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
        if import.glob {
            result.diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnsupportedSyntax {
                    feature: "wildcard imports",
                })
                .with_span(import.span),
            );
            result.entries.push(ResolvedImport {
                alias: import.alias.clone(),
                span: import.span,
                target: None,
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
                match resolve_path(&import.path, catalog, hosts) {
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
        });
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
        candidates.push(ImportTarget::Source(module.target(None)));
    }
    if let Some((parent, member)) = path.rsplit_once("::") {
        for module in catalog.paths.get(parent).into_iter().flatten() {
            match module.members.get(member).map(Vec::as_slice) {
                Some([item]) => candidates.push(ImportTarget::Source(module.target(Some(*item)))),
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

struct SourceCatalog<'a> {
    paths: BTreeMap<String, Vec<SourceCatalogEntry<'a>>>,
}

struct SourceCatalogEntry<'a> {
    source: &'a kagari_common::SourceFile,
    members: Arc<BTreeMap<String, Vec<ExportItem>>>,
}

impl<'a> SourceCatalog<'a> {
    fn new(
        modules: impl IntoIterator<Item = &'a LoweredModule>,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let mut paths = BTreeMap::<_, Vec<_>>::new();
        for module in modules {
            cancel.check()?;
            let mut members = BTreeMap::<_, Vec<_>>::new();
            for export in &module.module.exports {
                cancel.check()?;
                members
                    .entry(export.name.clone())
                    .or_default()
                    .push(export.item);
            }
            paths
                .entry(module.source.module_identity().to_string())
                .or_default()
                .push(SourceCatalogEntry {
                    source: &module.source,
                    members: Arc::new(members),
                });
        }
        Ok(Self { paths })
    }
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
