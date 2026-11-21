//! Mutable fixed-point preparation; immutable publication contains no pending edges.
use crate::imports::environment::{ImportEnvironment, ModuleScope};
use crate::{
    collections::{HashMap, HashSet},
    hir::{item::function::FunctionKind, ty::TypeKind},
    imports::{
        BindingCandidate, BindingOrigin, LocalName, ModuleGraph, ModuleImportFacts, ModuleNode,
        ModuleOrderError, NamespaceId, ResolvedTarget, SourceDeclRef, SourceItem, SourceUnit,
        bindings::LookupOutcome,
        catalog::{NamespaceCatalog, NamespaceTable},
        solver::{ImportSolveError, SolvedImports, SolverLimits, solve},
    },
    lower::LoweredModule,
    resolver::table::NameTable,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{PackageId, map::DefinitionContext, table::module::ModuleHandle},
    span::Span,
};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    identity::FileSpan,
    package::PackageEnvironment,
};
use kagari_types::{declaration::names::NameNamespace, visibility::Visibility};
use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
    sync::Arc,
};

impl ModuleGraph {
    /// Builds the source input set's declarations, import fixed point and immutable catalog.
    ///
    /// Seeds declarations once, then solves observed namespace dependencies.
    /// No draft is returned on cancellation, exhaustion or non-convergence.
    pub(crate) fn build<'a>(
        sources: impl IntoIterator<Item = &'a LoweredModule>,
        definitions: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<Self, ImportSolveError> {
        Self::build_with_limits(
            sources,
            definitions,
            &PackageEnvironment::default(),
            cancel,
            None,
        )
    }

    pub(crate) fn build_with_limits<'a>(
        sources: impl IntoIterator<Item = &'a LoweredModule>,
        definitions: &DefinitionContext,
        packages: &PackageEnvironment,
        cancel: &CancellationToken,
        limits: Option<SolverLimits>,
    ) -> Result<Self, ImportSolveError> {
        Self::build_with_environment(
            sources,
            definitions,
            packages,
            &Default::default(),
            cancel,
            limits,
        )
    }

    pub(crate) fn build_with_environment<'a>(
        sources: impl IntoIterator<Item = &'a LoweredModule>,
        definitions: &DefinitionContext,
        packages: &PackageEnvironment,
        environment: &ImportEnvironment,
        cancel: &CancellationToken,
        limits: Option<SolverLimits>,
    ) -> Result<Self, ImportSolveError> {
        let mut sources = sources.into_iter().collect::<Vec<_>>();
        sources.sort_by(|a, b| {
            a.source
                .module_identity()
                .cmp(b.source.module_identity())
                .then_with(|| a.source.id().cmp(&b.source.id()))
        });
        for module in &sources {
            cancel.check()?;
            for identity in iter::once(module.source.module_identity()).chain(
                environment
                    .scope(module.source.module_identity())
                    .into_iter()
                    .flat_map(|scope| &scope.dependencies),
            ) {
                definitions
                    .intern_module(identity)
                    .map_err(ImportSolveError::Identity)?;
            }
        }
        let mut base = NamespaceCatalog::new(definitions.snapshot(), packages.clone());
        base.environment = environment.clone();
        for module in &sources {
            let identity = module.source.module_identity();
            base.environment
                .modules
                .entry(identity.clone())
                .or_insert_with(|| ModuleScope {
                    packages: packages
                        .scope(&identity.package)
                        .cloned()
                        .unwrap_or_default(),
                    ..Default::default()
                });
        }
        for module in &sources {
            cancel.check()?;
            let unit = SourceUnit::of(module, &base.definitions);
            base.modules.entry(unit.module).or_default().push(unit);
            if !module.trait_references.is_empty() {
                base.registered_trait_access.insert(
                    unit.module,
                    module.trait_references.iter().map(|(id, _)| *id).collect(),
                );
            }
            base.module_dependencies.insert(
                unit.module,
                environment
                    .scope(module.source.module_identity())
                    .into_iter()
                    .flat_map(|scope| &scope.dependencies)
                    .map(|id| {
                        base.definitions
                            .lookup_module(id)
                            .expect("explicit module dependency")
                    })
                    .collect(),
            );
        }
        for package in packages.scopes().keys() {
            if !base
                .definitions
                .lookup_module_path(package, &[])
                .is_some_and(|root| base.modules.contains_key(&root))
            {
                return Err(ImportSolveError::Packages(format!(
                    "missing root of {}",
                    package.0
                )));
            }
        }
        for module in &sources {
            add_declarations(&mut base, module, cancel)?;
        }
        add_package_prefixes(&mut base, &sources, cancel)?;
        let SolvedImports {
            catalog,
            facts: resolved,
            work: _work,
        } = solve(&sources, base, cancel, limits)?;
        // A package hub retains the complete module set without a quadratic clique.
        // Normalized packages have an empty-path root; manually bound analysis inputs
        // use their first deterministic module as the same traversal anchor.
        let mut members: BTreeMap<PackageId, Vec<ModuleHandle>> = BTreeMap::new();
        let mut order = catalog.modules.keys().copied().collect::<Vec<_>>();
        order.sort_by(|a, b| catalog.identity(*a).cmp(catalog.identity(*b)));
        for identity in &order {
            members
                .entry(catalog.identity(*identity).package.clone())
                .or_default()
                .push(*identity);
        }
        let mut nodes = BTreeMap::new();
        let mut source_facts = HashMap::default();
        for (unit, mut facts) in resolved {
            cancel.check()?;
            let identity = &unit.module;
            let package = &members[&catalog.identity(*identity).package];
            let hub = &package[0];
            if identity == hub {
                facts
                    .dependencies
                    .extend(package.iter().filter(|member| *member != identity).cloned());
                if let Some(scope) = packages.scope(&catalog.identity(*identity).package) {
                    for target in scope.dependencies.values() {
                        facts.dependencies.push(members[target][0]);
                    }
                }
            } else {
                facts.dependencies.push(*hub);
            }
            facts
                .dependencies
                .sort_by(|a, b| catalog.identity(*a).cmp(catalog.identity(*b)));
            facts.dependencies.dedup();
            let units = &catalog.modules[identity];
            if units.len() > 1 {
                facts.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::DuplicateDeclaration {
                        name: catalog.identity(*identity).to_string(),
                    })
                    .with_span(Span::default()),
                );
            }
            let facts = Arc::new(facts);
            if unit == units[0] {
                nodes.insert(
                    *identity,
                    ModuleNode {
                        file: unit.file,
                        revision: unit.revision,
                        imports: facts.clone(),
                    },
                );
            }
            source_facts.insert(unit, facts);
        }
        Ok(Self {
            nodes,
            order,
            source_facts,
            catalog: Arc::new(catalog),
            #[cfg(test)]
            work: _work,
        })
    }

    /// Returns facts for an exact source unit, retaining duplicate logical modules separately.
    pub(crate) fn imports_for(&self, unit: &SourceUnit) -> Option<&Arc<ModuleImportFacts>> {
        self.source_facts.get(unit)
    }

    /// Finds the representative graph node for a logical identity, or `None` if absent.
    pub fn node(&self, module: &ModuleHandle) -> Option<&ModuleNode> {
        self.nodes.get(module)
    }

    /// Visits representative nodes in ordered logical-identity order.
    pub fn modules(&self) -> impl Iterator<Item = (&ModuleHandle, &ModuleNode)> {
        self.order
            .iter()
            .map(|module| (module, &self.nodes[module]))
    }

    /// Collects the root and transitive dependencies in sorted identity order.
    ///
    /// This is not a dependency-before-user topological order; a visited set terminates
    /// cycles. Every visited node must have no import diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a missing-node, foreign/later identity, invalid-import or cancellation error.
    pub fn reachable_order(
        &self,
        root: &ModuleHandle,
        cancel: &CancellationToken,
    ) -> Result<Vec<ModuleHandle>, ModuleOrderError> {
        if !self.nodes.contains_key(root) {
            return Err(ModuleOrderError::Missing(
                self.catalog
                    .definitions
                    .module_identity(*root)
                    .map_err(ModuleOrderError::Identity)?
                    .clone(),
            ));
        }
        let mut reachable = BTreeSet::new();
        let mut pending = vec![*root];
        while let Some(module) = pending.pop() {
            cancel.check().map_err(|_| ModuleOrderError::Cancelled)?;
            if !reachable.insert(module) {
                continue;
            }
            let node = self
                .nodes
                .get(&module)
                .ok_or_else(|| ModuleOrderError::Missing(self.catalog.identity(module).clone()))?;
            if !node.imports.diagnostics.is_empty() {
                return Err(ModuleOrderError::InvalidImports(
                    self.catalog.identity(module).clone(),
                ));
            }
            pending.extend(node.dependencies().iter().cloned());
        }
        let mut order = reachable.into_iter().collect::<Vec<_>>();
        order.sort_by(|a, b| self.catalog.identity(*a).cmp(self.catalog.identity(*b)));
        Ok(order)
    }
}

pub(super) fn source_location(module: &LoweredModule, span: Option<Span>) -> Option<FileSpan> {
    module.source_surface()?.span(span?)
}

pub(super) fn candidate(
    unit: &SourceUnit,
    target: Option<ResolvedTarget>,
    origin: BindingOrigin,
    visibility: Visibility,
    location: Option<FileSpan>,
) -> BindingCandidate {
    BindingCandidate {
        resolution: target.map_or(LookupOutcome::Absent, LookupOutcome::Resolved),
        support: Default::default(),
        origin,
        owner: unit.module,
        visibility,
        location,
    }
}

fn add_source(
    names: &mut NameTable,
    unit: &SourceUnit,
    name: &str,
    item: SourceItem,
    visibility: Visibility,
    location: Option<FileSpan>,
) {
    if let Some(name) = LocalName::new(name) {
        let source = SourceDeclRef { unit: *unit, item };
        names.add(
            item.namespace(),
            name,
            candidate(
                unit,
                Some(ResolvedTarget::Source(source)),
                BindingOrigin::Declaration(source),
                visibility,
                location,
            ),
        );
    }
}

fn add_declarations(
    catalog: &mut NamespaceCatalog,
    module: &LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    let unit = SourceUnit::of(module, &catalog.definitions);
    let mut names = NameTable::for_unit(unit);
    for item in &module.module.functions {
        cancel.check()?;
        if item.kind == FunctionKind::User {
            add_source(
                &mut names,
                &unit,
                &item.name,
                SourceItem::Function(item.id),
                item.visibility,
                source_location(module, item.origin.source_span()),
            );
        }
    }
    for item in &module.module.consts {
        cancel.check()?;
        if item.owner.is_none() {
            add_source(
                &mut names,
                &unit,
                &item.name,
                SourceItem::Const(item.id),
                item.visibility,
                source_location(module, item.origin.source_span()),
            );
        }
    }
    for item in &module.module.statics {
        cancel.check()?;
        add_source(
            &mut names,
            &unit,
            &item.name,
            SourceItem::Static(item.id),
            item.visibility,
            source_location(module, item.origin.source_span()),
        );
    }
    for item in &module.module.structs {
        cancel.check()?;
        add_source(
            &mut names,
            &unit,
            &item.name,
            SourceItem::Struct(item.id),
            item.visibility,
            source_location(module, item.origin.source_span()),
        );
    }
    for item in &module.module.enums {
        cancel.check()?;
        add_source(
            &mut names,
            &unit,
            &item.name,
            SourceItem::Enum(item.id),
            item.visibility,
            source_location(module, item.origin.source_span()),
        );
    }
    for item in &module.module.native_types {
        cancel.check()?;
        add_source(
            &mut names,
            &unit,
            &item.name,
            SourceItem::NativeType(item.id),
            item.visibility,
            source_location(module, item.origin.source_span()),
        );
    }
    for item in &module.module.type_aliases {
        cancel.check()?;
        add_source(
            &mut names,
            &unit,
            &item.name,
            SourceItem::TypeAlias(item.id),
            item.visibility,
            source_location(module, item.origin.source_span()),
        );
    }
    for item in &module.module.traits {
        cancel.check()?;
        add_source(
            &mut names,
            &unit,
            &item.name,
            SourceItem::Trait(item.id),
            item.visibility,
            source_location(module, item.origin.source_span()),
        );
    }
    for item in &module.module.modules {
        cancel.check()?;
        let child = catalog.definitions.module_child(unit.module, &item.name);
        let targets = child
            .and_then(|identity| catalog.modules.get(&identity))
            .map_or_else(
                || vec![None],
                |units| {
                    units
                        .iter()
                        .map(|child| Some(ResolvedTarget::Namespace(NamespaceId::Module(*child))))
                        .collect()
                },
            );
        if let Some(name) = LocalName::new(&item.name) {
            for target in targets {
                cancel.check()?;
                names.add(
                    NameNamespace::Type,
                    name.clone(),
                    candidate(
                        &unit,
                        target,
                        BindingOrigin::ModuleDeclaration {
                            unit,
                            module: item.id,
                        },
                        item.visibility,
                        source_location(module, item.origin.source_span()),
                    ),
                );
            }
        }
    }
    for item in &module.module.impls {
        names.insert_impl(item.id);
    }
    let mut associated = BTreeMap::<String, (SourceItem, Visibility, NameTable)>::new();
    for item in &module.module.structs {
        cancel.check()?;
        associated.insert(
            item.name.clone(),
            (
                SourceItem::Struct(item.id),
                item.visibility,
                NameTable::default(),
            ),
        );
    }
    for item in &module.module.native_types {
        cancel.check()?;
        associated.insert(
            item.name.clone(),
            (
                SourceItem::NativeType(item.id),
                item.visibility,
                NameTable::default(),
            ),
        );
    }
    for item in &module.module.enums {
        cancel.check()?;
        let mut table = NameTable::default();
        for variant in &item.variants {
            add_source(
                &mut table,
                &unit,
                &variant.name,
                SourceItem::Variant(variant.id),
                item.visibility,
                source_location(
                    module,
                    variant
                        .origin
                        .source_span()
                        .map(|span| variant.name_span.unwrap_or(span)),
                ),
            );
        }
        associated.insert(
            item.name.clone(),
            (SourceItem::Enum(item.id), item.visibility, table),
        );
    }
    for implementation in &module.module.impls {
        cancel.check()?;
        if implementation.trait_ref.is_some() {
            continue;
        }
        let Some(reference) = implementation.for_type else {
            continue;
        };
        let (TypeKind::Named(name) | TypeKind::Generic { name, .. }) =
            &module.module.type_ref(reference).kind
        else {
            continue;
        };
        let Some((_, _, table)) = associated.get_mut(name) else {
            continue;
        };
        for method in &implementation.methods {
            let Some(function) = module
                .module
                .functions
                .iter()
                .find(|f| f.id == method.function)
            else {
                continue;
            };
            add_source(
                table,
                &unit,
                &method.name,
                SourceItem::Function(function.id),
                function.visibility,
                source_location(module, function.origin.source_span()),
            );
        }
    }
    for (_, (item, _, table)) in associated {
        catalog.namespaces.insert(
            NamespaceId::Associated(SourceDeclRef { unit, item }),
            NamespaceTable {
                owner: unit.module,
                names: Arc::new(table),
                glob_allowed: matches!(item, SourceItem::Enum(_)),
            },
        );
    }
    for entry in names
        .entries
        .values_mut()
        .flat_map(|slots| slots.values_mut())
    {
        entry
            .strong
            .sort_by_key(|candidate| candidate.location.map_or(0, |span| span.range.start));
    }
    catalog.namespaces.insert(
        NamespaceId::Module(unit),
        NamespaceTable {
            owner: unit.module,
            names: Arc::new(names),
            glob_allowed: true,
        },
    );
    Ok(())
}

fn add_package_prefixes(
    catalog: &mut NamespaceCatalog,
    sources: &[&LoweredModule],
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    let package_units = sources
        .iter()
        .map(|module| SourceUnit::of(module, &catalog.definitions))
        .collect::<HashSet<_>>();
    for module in sources {
        let unit = SourceUnit::of(module, &catalog.definitions);
        let portable = module.source.module_identity();
        for len in 0..portable.path.len() {
            cancel.check()?;
            let identity = catalog
                .definitions
                .lookup_module_path(&portable.package, &portable.path[..len])
                .expect("registered prefix");
            let namespace = match catalog.modules.get(&identity).map(Vec::as_slice) {
                Some([parent]) if package_units.contains(parent) => NamespaceId::Module(*parent),
                Some(_) => continue,
                None => NamespaceId::InstalledPrefix(identity),
            };
            let child = catalog
                .definitions
                .module_child(identity, &portable.path[len])
                .expect("registered child");
            let target = catalog
                .modules
                .get(&child)
                .and_then(|units| units.first())
                .map_or_else(
                    || NamespaceId::InstalledPrefix(child),
                    |unit| NamespaceId::Module(*unit),
                );
            let table = catalog
                .namespaces
                .entry(namespace)
                .or_insert_with(|| NamespaceTable {
                    owner: identity,
                    names: Arc::new(NameTable::default()),
                    glob_allowed: true,
                });
            let names = Arc::make_mut(&mut table.names);
            let name = LocalName::new(&portable.path[len]).expect("installed module component");
            if !names.entries.contains_key(&name) {
                names.add(
                    NameNamespace::Type,
                    name,
                    candidate(
                        &unit,
                        Some(ResolvedTarget::Namespace(target)),
                        BindingOrigin::Package(portable.package.clone()),
                        Visibility::Public,
                        None,
                    ),
                );
            }
        }
    }
    Ok(())
}
