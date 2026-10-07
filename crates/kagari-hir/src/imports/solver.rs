//! Import solver work accounting and completion boundary.
use crate::{
    hir::item::module::ImportLeaf,
    host::HostDeclarations,
    imports::{
        BindingOrigin, DirectiveId, DirectiveResolution, LocalName, ModuleImportFacts, NamespaceId,
        SourceUnit,
        bindings::LookupOutcome,
        builder::{candidate, location},
        catalog::NamespaceCatalog,
        resolve::{ResolutionInputs, resolve_imports},
    },
    lower::LoweredModule,
    resolver::table::NameTable,
};
use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_source::identity::FileSpan;
use kagari_types::declaration::names::NameNamespace;
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    sync::Arc,
};

/// Lookup dependencies are distinct from module linking/reachability edges.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Observation {
    Namespace(NamespaceId),
    /// Package aliases and missing physical prefixes belong to the immutable input universe.
    InputUniverse,
}

/// A safety bound, not a completeness claim for every well-formed program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolverLimits {
    pub module_visits: usize,
    pub candidate_work: usize,
}

impl SolverLimits {
    fn for_inputs(sources: &[&LoweredModule], base: &NamespaceCatalog) -> Self {
        let size = sources
            .len()
            .saturating_add(
                sources
                    .iter()
                    .map(|m| m.module.imports.len())
                    .sum::<usize>(),
            )
            .saturating_add(
                base.namespaces
                    .values()
                    .map(|table| table.names.entries.len())
                    .sum::<usize>(),
            );
        let module_visits = size.saturating_mul(size).saturating_mul(4).max(1024);
        Self {
            module_visits,
            candidate_work: module_visits.saturating_mul(size.max(1)),
        }
    }
}

/// A failed solve never publishes a graph or authorizes code generation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportSolveError {
    #[error("import resolution cancelled")]
    Cancelled,
    #[error(
        "import resolution exhausted its work bound: {work:?}; limits: {limits:?}; imports: {imports:?}"
    )]
    Exhausted {
        work: SolverWork,
        limits: SolverLimits,
        imports: Vec<FileSpan>,
    },
    #[error("import resolution did not converge: {work:?}; imports: {imports:?}")]
    NonConvergent {
        work: SolverWork,
        imports: Vec<FileSpan>,
    },
}

impl From<Cancelled> for ImportSolveError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

/// Counts module evaluations and the binding contributions they produce.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SolverWork {
    pub module_visits: usize,
    pub changed_entries: usize,
    pub candidate_work: usize,
}

pub(super) struct SolvedImports {
    pub catalog: NamespaceCatalog,
    pub facts: HashMap<SourceUnit, ModuleImportFacts>,
    pub work: SolverWork,
}

/// Exact, bounded history; hash collisions cannot be mistaken for a repeated state.
#[derive(PartialEq, Eq)]
struct Checkpoint {
    facts: Vec<Arc<ModuleImportFacts>>,
    observations: Vec<HashSet<Observation>>,
    queue: BTreeSet<usize>,
    closed: HashSet<(DirectiveId, NameNamespace)>,
}

/// Evaluate only modules observing a changed namespace. Drafts never escape this owner.
///
/// Selection is not monotone: releasing a pending strong slot can expose a glob,
/// which can later resolve that strong slot. At quiescence every pending dependency
/// is inside the retained pending group or an already stable input. Close the whole
/// group simultaneously, then re-evaluate its members and their observers. This is
/// a closed union of SCCs; an empty queue alone is never interpreted as absence.
/// Closing is finite (at most two keys per directive). Newly exposed seeds still
/// resolve normally; a closed key only permits absence when lookup remains pending.
/// Publication requires a second fixed point without any pending outcomes/barriers.
/// Nonmonotone selections may still cycle; exact bounded history and a separate work
/// limit produce explicit failure rather than silently publishing the last draft.
pub(super) fn solve(
    sources: &[&LoweredModule],
    base: NamespaceCatalog,
    hosts: &HostDeclarations,
    cancel: &CancellationToken,
    limits: Option<SolverLimits>,
) -> Result<SolvedImports, ImportSolveError> {
    let limits = limits.unwrap_or_else(|| SolverLimits::for_inputs(sources, &base));
    let units = sources
        .iter()
        .map(|module| SourceUnit::of(module))
        .collect::<Vec<_>>();
    let mut catalog = base.clone();
    let mut facts = Vec::with_capacity(sources.len());
    for (module, unit) in sources.iter().zip(&units) {
        cancel.check()?;
        let ns = NamespaceId::Module(unit.clone());
        let mut scope = (*catalog.namespaces[&ns].names).clone();
        reserve(module, &mut scope, cancel)?;
        let scope = Arc::new(scope);
        catalog
            .namespaces
            .get_mut(&ns)
            .expect("seeded module")
            .names = scope.clone();
        facts.push(Arc::new(ModuleImportFacts {
            scope,
            ..Default::default()
        }));
    }
    let mut queue = (0..sources.len()).collect::<BTreeSet<_>>();
    let mut observed = vec![HashSet::new(); sources.len()];
    let mut watchers = HashMap::<Observation, BTreeSet<usize>>::new();
    let mut closed = HashSet::new();
    let mut history = VecDeque::<Checkpoint>::new();
    let mut work = SolverWork::default();
    loop {
        while let Some(index) = queue.pop_first() {
            cancel.check()?;
            if work.module_visits >= limits.module_visits {
                return Err(ImportSolveError::Exhausted {
                    work,
                    limits,
                    imports: failure_sites(sources, [index]),
                });
            }
            let observations = RefCell::new(HashSet::new());
            let next = resolve_imports(
                sources[index],
                &ResolutionInputs {
                    base: &base,
                    catalog: &catalog,
                    sources,
                    hosts,
                    cancel,
                    closed: &closed,
                    observations: &observations,
                },
            )?;
            let ns = NamespaceId::Module(units[index].clone());
            let changed = catalog.namespaces[&ns].names != next.scope;
            work.evaluated(&next.scope, changed);
            if work.candidate_work > limits.candidate_work {
                return Err(ImportSolveError::Exhausted {
                    work,
                    limits,
                    imports: failure_sites(sources, [index]),
                });
            }
            for old in observed[index].drain() {
                if let Some(readers) = watchers.get_mut(&old) {
                    readers.remove(&index);
                }
            }
            observed[index] = observations.into_inner();
            for observation in &observed[index] {
                watchers
                    .entry(observation.clone())
                    .or_default()
                    .insert(index);
            }
            if changed {
                catalog
                    .namespaces
                    .get_mut(&ns)
                    .expect("seeded namespace")
                    .names = next.scope.clone();
                if let Some(readers) = watchers.get(&Observation::Namespace(ns)) {
                    queue.extend(readers);
                }
            }
            facts[index] = Arc::new(next);
            if work.module_visits.is_multiple_of(sources.len().max(1)) && !queue.is_empty() {
                let state = Checkpoint {
                    facts: facts.clone(),
                    observations: observed.clone(),
                    queue: queue.clone(),
                    closed: closed.clone(),
                };
                if history.contains(&state) {
                    return Err(ImportSolveError::NonConvergent {
                        work,
                        imports: failure_sites(sources, queue.iter().copied().chain([index])),
                    });
                }
                if history.len() == 8 {
                    history.pop_front();
                }
                history.push_back(state);
            }
        }
        cancel.check()?;
        let mut pending_owners = BTreeSet::new();
        for (index, module) in facts.iter().enumerate() {
            for directive in &module.directives {
                cancel.check()?;
                let outcomes = match &directive.resolution {
                    DirectiveResolution::Named(outcomes) => outcomes.iter().collect::<Vec<_>>(),
                    DirectiveResolution::Glob(outcome) => vec![(NameNamespace::Type, outcome)],
                };
                for (namespace, outcome) in outcomes {
                    if *outcome == LookupOutcome::Pending {
                        pending_owners.insert(index);
                        if closed.insert((directive.id.clone(), namespace)) {
                            queue.insert(index);
                        }
                    }
                }
            }
        }
        if pending_owners.is_empty() {
            // Directive settlement must agree with the actual published tiers.
            if let Some((index, _)) = facts.iter().enumerate().find(|(_, module)| {
                !module.scope.pending_globs.is_empty()
                    || module
                        .scope
                        .entries
                        .values()
                        .flat_map(|slots| slots.iter())
                        .any(|(_, entry)| {
                            entry
                                .strong
                                .iter()
                                .chain(&entry.globs)
                                .chain(&entry.implicit)
                                .any(|binding| binding.resolution == LookupOutcome::Pending)
                        })
            }) {
                return Err(ImportSolveError::NonConvergent {
                    work,
                    imports: failure_sites(sources, [index]),
                });
            }
            break;
        }
        if queue.is_empty() {
            return Err(ImportSolveError::NonConvergent {
                work,
                imports: failure_sites(sources, pending_owners),
            });
        }
    }
    drop(history);
    Ok(SolvedImports {
        catalog,
        facts: units
            .into_iter()
            .zip(facts.into_iter().map(Arc::unwrap_or_clone))
            .collect(),
        work,
    })
}

fn reserve(
    module: &LoweredModule,
    names: &mut NameTable,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    let unit = SourceUnit::of(module);
    for (slot, import) in module.module.imports.iter().enumerate() {
        cancel.check()?;
        let id = DirectiveId {
            unit: unit.clone(),
            slot: u32::try_from(slot).expect("import capacity"),
        };
        let origin = match import.kind {
            ImportLeaf::Named { .. } => BindingOrigin::NamedImport(id),
            ImportLeaf::Glob => BindingOrigin::GlobImport(id),
        };
        let mut binding = candidate(
            &unit,
            None,
            origin,
            import.visibility,
            Some(location(module, import.span)),
        );
        binding.resolution = LookupOutcome::Pending;
        if let Some(name) = import.local_name().and_then(LocalName::new) {
            for namespace in NameNamespace::ALL {
                if module
                    .native_import_namespaces
                    .get(&slot)
                    .is_none_or(|selected| *selected == namespace)
                {
                    names.add(namespace, name.clone(), binding.clone());
                }
            }
        } else {
            names.pending_globs.push(binding);
        }
    }
    Ok(())
}

impl SolverWork {
    pub(super) fn evaluated(&mut self, names: &NameTable, changed: bool) {
        self.module_visits = self.module_visits.saturating_add(1);
        self.changed_entries += usize::from(changed);
        self.candidate_work = self.candidate_work.saturating_add(
            names
                .entries
                .values()
                .flat_map(|slots| slots.iter())
                .map(|(_, entry)| entry.strong.len() + entry.globs.len() + entry.implicit.len())
                .sum::<usize>(),
        );
    }
}

/// Bound diagnostic payloads while retaining physical sites of responsible imports.
fn failure_sites(
    sources: &[&LoweredModule],
    indices: impl IntoIterator<Item = usize>,
) -> Vec<FileSpan> {
    indices
        .into_iter()
        .flat_map(|index| {
            let module = sources[index];
            module
                .module
                .imports
                .iter()
                .map(move |import| location(module, import.span))
        })
        .take(16)
        .collect()
}
