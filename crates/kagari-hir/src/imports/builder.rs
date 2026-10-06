//! Mutable fixed-point preparation; immutable publication contains no pending edges.
use crate::{
    hir::{
        item::{function::FunctionKind, storage::ExportItem},
        ty::TypeKind,
    },
    host::HostDeclarations,
    imports::{
        BindingCandidate, BindingOrigin, DirectiveId, DirectiveResolution, ImportDirective,
        ImportKind, LocalName, ModuleGraph, ModuleImportFacts, ModuleNode, ModuleOrderError,
        NamespaceId, ResolvedTarget, SourceDeclRef, SourceItem, SourceUnit,
        catalog::{
            LookupContext, LookupHit, LookupResult, NamespaceCatalog, NamespaceResult,
            NamespaceTable,
        },
    },
    lower::LoweredModule,
    resolver::table::NameTable,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::ModuleIdentity,
    span::Span,
};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    identity::FileSpan,
};
use kagari_types::visibility::Visibility;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

impl ModuleGraph {
    pub(crate) fn build<'a>(
        sources: impl IntoIterator<Item = &'a LoweredModule>,
        hosts: &HostDeclarations,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let sources = sources.into_iter().collect::<Vec<_>>();
        let mut base = NamespaceCatalog::default();
        for module in &sources {
            cancel.check()?;
            let unit = SourceUnit::of(module);
            base.modules
                .entry(unit.module.clone())
                .or_default()
                .push(unit);
            if let Some(alias) = &module.native_package_alias {
                base.package_aliases
                    .entry(alias.clone())
                    .or_default()
                    .insert(module.source.module_identity().package.clone());
            }
        }
        for module in &sources {
            add_declarations(&mut base, module, cancel)?;
        }
        add_installed_prefixes(&mut base, &sources, cancel)?;
        let mut catalog = base.clone();
        let mut resolved = HashMap::new();
        // SA2/SA3 scheduling and round policy are intentionally unchanged.
        for _ in 0..=base.modules.len() * 2 + 1 {
            let mut next = base.clone();
            resolved.clear();
            for module in &sources {
                cancel.check()?;
                let facts = resolve_imports(module, &base, &catalog, &sources, hosts, cancel)?;
                let ns = NamespaceId::Module(SourceUnit::of(module));
                next.namespaces
                    .get_mut(&ns)
                    .expect("module namespace")
                    .names = facts.scope.clone();
                resolved.insert(SourceUnit::of(module), facts);
            }
            let stable = next == catalog;
            catalog = next;
            if stable {
                break;
            }
        }
        let mut nodes = BTreeMap::new();
        let mut source_facts = HashMap::new();
        for (unit, mut facts) in resolved {
            cancel.check()?;
            let identity = &unit.module;
            let units = &catalog.modules[identity];
            if units.len() > 1 {
                facts.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::DuplicateDeclaration {
                        name: identity.to_string(),
                    })
                    .with_span(Span::default()),
                );
            }
            let facts = Arc::new(facts);
            if unit == units[0] {
                nodes.insert(
                    identity.clone(),
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
            source_facts,
            catalog: Arc::new(catalog),
        })
    }
    pub(crate) fn imports_for(&self, unit: &SourceUnit) -> Option<&Arc<ModuleImportFacts>> {
        self.source_facts.get(unit)
    }
    pub fn node(&self, module: &ModuleIdentity) -> Option<&ModuleNode> {
        self.nodes.get(module)
    }
    pub fn modules(&self) -> impl Iterator<Item = (&ModuleIdentity, &ModuleNode)> {
        self.nodes.iter()
    }
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
            let node = self
                .nodes
                .get(&module)
                .ok_or_else(|| ModuleOrderError::Missing(module.clone()))?;
            if !node.imports.diagnostics.is_empty() {
                return Err(ModuleOrderError::InvalidImports(module));
            }
            pending.extend(node.dependencies().iter().cloned());
        }
        Ok(reachable.into_iter().collect())
    }
}
fn location(module: &LoweredModule, span: Span) -> FileSpan {
    module.source.span(span).unwrap_or(FileSpan {
        file: module.source.origin_id(),
        revision: module.source.revision(),
        range: span,
    })
}
fn candidate(
    unit: &SourceUnit,
    target: Option<ResolvedTarget>,
    origin: BindingOrigin,
    visibility: Visibility,
    location: Option<FileSpan>,
) -> BindingCandidate {
    BindingCandidate {
        target,
        origin,
        owner: unit.module.clone(),
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
    location: FileSpan,
) {
    if let Some(name) = LocalName::new(name) {
        let source = SourceDeclRef {
            unit: unit.clone(),
            item,
        };
        names.add(
            name,
            candidate(
                unit,
                Some(ResolvedTarget::Source(source.clone())),
                BindingOrigin::Declaration(source),
                visibility,
                Some(location),
            ),
        );
    }
}
fn add_declarations(
    catalog: &mut NamespaceCatalog,
    module: &LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    let unit = SourceUnit::of(module);
    let mut names = NameTable::for_unit(unit.clone());
    for item in &module.module.functions {
        cancel.check()?;
        if item.kind == FunctionKind::User {
            add_source(
                &mut names,
                &unit,
                &item.name,
                SourceItem::Function(item.id),
                item.visibility,
                location(module, module.source_map.function_span(item.id)),
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
                location(module, module.source_map.const_span(item.id)),
            );
        }
    }
    for item in &module.module.structs {
        cancel.check()?;
        add_source(
            &mut names,
            &unit,
            &item.name,
            SourceItem::Struct(item.id),
            item.visibility,
            location(module, module.source_map.struct_span(item.id)),
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
            location(module, module.source_map.enum_span(item.id)),
        );
    }
    for item in &module.module.opaque_types {
        cancel.check()?;
        add_source(
            &mut names,
            &unit,
            &item.name,
            SourceItem::OpaqueType(item.id),
            item.visibility,
            location(module, module.source_map.opaque_type_span(item.id)),
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
            location(module, module.source_map.trait_span(item.id)),
        );
    }
    for item in &module.module.modules {
        cancel.check()?;
        let mut identity = unit.module.clone();
        identity.path.push(item.name.clone());
        let targets = catalog.modules.get(&identity).map_or_else(
            || vec![None],
            |units| {
                units
                    .iter()
                    .map(|child| {
                        Some(ResolvedTarget::Namespace(NamespaceId::Module(
                            child.clone(),
                        )))
                    })
                    .collect()
            },
        );
        if let Some(name) = LocalName::new(&item.name) {
            for target in targets {
                cancel.check()?;
                names.add(
                    name.clone(),
                    candidate(
                        &unit,
                        target,
                        BindingOrigin::ModuleDeclaration {
                            unit: unit.clone(),
                            module: item.id,
                        },
                        item.visibility,
                        Some(location(module, module.source_map.module_span(item.id))),
                    ),
                );
            }
        }
    }
    for item in &module.module.impls {
        names.insert_impl(item.id);
    }
    for export in &module.module.exports {
        if let ExportItem::Variant(id) = export.item {
            add_source(
                &mut names,
                &unit,
                &export.name,
                SourceItem::Variant(id),
                Visibility::Public,
                location(module, module.source_map.variant_span(id)),
            );
        }
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
    for item in &module.module.opaque_types {
        cancel.check()?;
        associated.insert(
            item.name.clone(),
            (
                SourceItem::OpaqueType(item.id),
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
                location(module, module.source_map.variant_span(variant.id)),
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
                location(module, module.source_map.function_span(function.id)),
            );
        }
    }
    for (_, (item, _, table)) in associated {
        catalog.namespaces.insert(
            NamespaceId::Associated(SourceDeclRef {
                unit: unit.clone(),
                item,
            }),
            NamespaceTable {
                owner: unit.module.clone(),
                names: Arc::new(table),
                glob_allowed: matches!(item, SourceItem::Enum(_)),
            },
        );
    }
    for entry in names.entries.values_mut() {
        entry
            .strong
            .sort_by_key(|candidate| candidate.location.map_or(0, |span| span.range.start));
    }
    catalog.namespaces.insert(
        NamespaceId::Module(unit.clone()),
        NamespaceTable {
            owner: unit.module.clone(),
            names: Arc::new(names),
            glob_allowed: true,
        },
    );
    Ok(())
}
fn add_installed_prefixes(
    catalog: &mut NamespaceCatalog,
    sources: &[&LoweredModule],
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    for module in sources.iter().filter(|m| m.registered_native_api) {
        let unit = SourceUnit::of(module);
        for len in 0..unit.module.path.len() {
            cancel.check()?;
            let identity = ModuleIdentity {
                package: unit.module.package.clone(),
                path: unit.module.path[..len].to_vec(),
            };
            if catalog.modules.contains_key(&identity) {
                continue;
            }
            let mut child = identity.clone();
            child.path.push(unit.module.path[len].clone());
            let target = catalog
                .modules
                .get(&child)
                .and_then(|units| units.first())
                .map_or_else(
                    || NamespaceId::InstalledPrefix(child),
                    |unit| NamespaceId::Module(unit.clone()),
                );
            let table = catalog
                .namespaces
                .entry(NamespaceId::InstalledPrefix(identity.clone()))
                .or_insert_with(|| NamespaceTable {
                    owner: identity.clone(),
                    names: Arc::new(NameTable::default()),
                    glob_allowed: true,
                });
            let names = Arc::make_mut(&mut table.names);
            let name = LocalName::new(&unit.module.path[len]).expect("installed module component");
            if !names.entries.contains_key(&name) {
                names.add(
                    name,
                    candidate(
                        &unit,
                        Some(ResolvedTarget::Namespace(target)),
                        BindingOrigin::Package(unit.module.package.clone()),
                        Visibility::Public,
                        None,
                    ),
                );
            }
        }
    }
    Ok(())
}
fn resolve_imports(
    module: &LoweredModule,
    base: &NamespaceCatalog,
    catalog: &NamespaceCatalog,
    sources: &[&LoweredModule],
    hosts: &HostDeclarations,
    cancel: &CancellationToken,
) -> Result<ModuleImportFacts, Cancelled> {
    let unit = SourceUnit::of(module);
    let ctx = LookupContext {
        importer: &unit.module,
        hosts,
    };
    let mut names = (*base.namespaces[&NamespaceId::Module(unit.clone())].names).clone();
    let mut result = ModuleImportFacts::default();
    let mut dependencies = BTreeSet::from_iter(module.native_dependencies.iter().cloned());
    for m in sources {
        result
            .array_interfaces
            .extend(m.native_array_interfaces.clone());
    }
    // Reserve every explicit name before resolution, so a failed strong import
    // cannot expose a weaker glob, prelude or package candidate.
    for (slot, import) in module.module.imports.iter().enumerate() {
        cancel.check()?;
        let id = DirectiveId {
            unit: unit.clone(),
            slot: u32::try_from(slot).expect("import capacity"),
        };
        if !import.glob
            && let Some(name) = LocalName::new(&import.alias)
        {
            names.add(
                name,
                candidate(
                    &unit,
                    None,
                    BindingOrigin::NamedImport(id.clone()),
                    import.visibility,
                    Some(location(module, import.span)),
                ),
            );
        }
        result.directives.push(ImportDirective {
            id,
            path: import.path.clone(),
            kind: if import.glob {
                ImportKind::Glob
            } else {
                ImportKind::Named {
                    alias: import
                        .alias_explicit
                        .then(|| LocalName::new(&import.alias))
                        .flatten(),
                }
            },
            span: location(module, import.span),
            root_span: location(module, import.root_span),
            visibility: import.visibility,
            resolution: DirectiveResolution::Pending,
            direct_dependencies: BTreeSet::new(),
        });
    }
    for directive in &mut result.directives {
        cancel.check()?;
        let path = normalize_import_path(&directive.path, &unit.module);
        catalog.path_dependencies(&path, &mut directive.direct_dependencies);
        for (prefix, span) in module.source_map.import_path(directive.id.slot as usize) {
            cancel.check()?;
            let path = normalize_import_path(prefix, &unit.module);
            if let Some(mut hit) = catalog.absolute(&ctx, &path, cancel)?.hit() {
                hit.via.insert(
                    0,
                    if matches!(directive.kind, ImportKind::Glob) {
                        BindingOrigin::GlobImport(directive.id.clone())
                    } else {
                        BindingOrigin::NamedImport(directive.id.clone())
                    },
                );
                result.path_hits.push((location(module, *span), hit));
            }
        }
        let lookup = catalog.absolute(&ctx, &path, cancel)?;
        match lookup {
            LookupResult::Found(hit)
                if matches!(directive.kind, ImportKind::Glob)
                    || reexport_allowed(
                        catalog,
                        &hit.target,
                        directive.visibility,
                        &unit.module,
                    ) =>
            {
                add_dependencies(&hit, &mut directive.direct_dependencies);
                directive.resolution = DirectiveResolution::Resolved(hit.target.clone());
                match &directive.kind {
                    ImportKind::Named { .. } => {
                        let Some(name) = LocalName::new(
                            &module.module.imports[directive.id.slot as usize].alias,
                        ) else {
                            continue;
                        };
                        let entry = names.entries.get_mut(&name).expect("reserved import");
                        for c in &mut entry.strong {
                            if c.origin == BindingOrigin::NamedImport(directive.id.clone()) {
                                c.target = Some(hit.target.clone());
                            }
                        }
                    }
                    ImportKind::Glob => {
                        let ns = catalog.namespace_of(&ctx, &hit.target, cancel)?;
                        let members = match ns {
                            NamespaceResult::Found(NamespaceId::Host(id)) => hosts
                                .members_of_module(id)
                                .into_iter()
                                .map(|(name, _)| name)
                                .collect::<Vec<_>>(),
                            NamespaceResult::Found(ref ns)
                                if catalog.namespaces.get(ns).is_some_and(|t| t.glob_allowed) =>
                            {
                                catalog.namespaces[ns]
                                    .names
                                    .entries
                                    .keys()
                                    .map(|n| n.as_str().to_owned())
                                    .collect()
                            }
                            _ => {
                                result.diagnostics.push(
                                    Diagnostic::error(DiagnosticKind::InvalidGlobTarget {
                                        path: directive.path.clone(),
                                    })
                                    .with_span(directive.span.range),
                                );
                                vec![]
                            }
                        };
                        if let NamespaceResult::Found(ns) = ns {
                            for name in members {
                                cancel.check()?;
                                if let Some(member) =
                                    catalog.lookup_member(&ctx, &ns, &name, cancel)?.hit()
                                    && reexport_allowed(
                                        catalog,
                                        &member.target,
                                        directive.visibility,
                                        &unit.module,
                                    )
                                {
                                    names.add(
                                        LocalName::new(&name).expect("member name"),
                                        candidate(
                                            &unit,
                                            Some(member.target),
                                            BindingOrigin::GlobImport(directive.id.clone()),
                                            directive.visibility,
                                            Some(directive.span),
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            other => {
                let kind = match other {
                    LookupResult::Ambiguous(_) => {
                        directive.resolution = DirectiveResolution::Ambiguous;
                        DiagnosticKind::AmbiguousImport { path }
                    }
                    LookupResult::Found(_) | LookupResult::Inaccessible(_) => {
                        directive.resolution = DirectiveResolution::Unresolved;
                        DiagnosticKind::ImportNotPublic { path }
                    }
                    _ => {
                        directive.resolution = DirectiveResolution::Unresolved;
                        DiagnosticKind::UnknownName { name: path }
                    }
                };
                result
                    .diagnostics
                    .push(Diagnostic::error(kind).with_span(directive.span.range));
            }
        }
        dependencies.extend(directive.direct_dependencies.iter().cloned());
    }
    for (name, entry) in &names.entries {
        if entry.strong.len() > 1
            || (entry.strong.is_empty()
                && matches!(
                    NameTable::select(&entry.globs, false),
                    LookupResult::Ambiguous(_)
                ))
        {
            let imported = if entry.strong.is_empty() {
                !entry.globs.is_empty()
            } else {
                entry
                    .strong
                    .iter()
                    .any(|c| matches!(c.origin, BindingOrigin::NamedImport(_)))
            };
            if imported {
                result.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::DuplicateImport {
                        name: name.as_str().into(),
                    })
                    .with_span(
                        entry
                            .strong
                            .last()
                            .or(entry.globs.last())
                            .and_then(|c| c.location)
                            .map_or(Span::default(), |s| s.range),
                    ),
                );
            }
        }
        if entry.strong.len() > 1 {
            for c in &entry.strong {
                if let BindingOrigin::NamedImport(id) = &c.origin {
                    result.directives[id.slot as usize].resolution = DirectiveResolution::Ambiguous;
                }
            }
        }
        for c in &entry.strong {
            if matches!(c.origin, BindingOrigin::ModuleDeclaration { .. }) {
                if let Some(ResolvedTarget::Namespace(NamespaceId::Module(child))) = &c.target {
                    dependencies.insert(child.module.clone());
                } else {
                    result.diagnostics.push(
                        Diagnostic::error(DiagnosticKind::UnknownName {
                            name: name.as_str().into(),
                        })
                        .with_span(c.location.map_or(Span::default(), |s| s.range)),
                    );
                }
            }
        }
    }
    {
        for m in sources.iter().filter(|m| m.registered_native_api) {
            let installed = SourceUnit::of(m);
            if !module.registered_native_api {
                dependencies.insert(installed.module.clone());
            }
            let alias = m
                .native_package_alias
                .as_deref()
                .unwrap_or(&installed.module.package.0);
            if let Some(name) = LocalName::new(alias) {
                let ns = NamespaceId::InstalledPrefix(ModuleIdentity {
                    package: installed.module.package.clone(),
                    path: vec![],
                });
                let c = candidate(
                    &unit,
                    Some(ResolvedTarget::Namespace(ns)),
                    BindingOrigin::Package(installed.module.package.clone()),
                    Visibility::Private,
                    None,
                );
                let entry = names.entries.entry(name.clone()).or_default();
                if !entry.implicit.contains(&c) {
                    names.add(name, c);
                }
            }
        }
    }
    if !module.registered_native_api {
        let preludes = sources
            .iter()
            .filter(|m| m.native_prelude)
            .collect::<Vec<_>>();
        if let [prelude] = preludes.as_slice() {
            let ns = NamespaceId::Module(SourceUnit::of(prelude));
            for name in catalog.namespaces[&ns].names.entries.keys() {
                cancel.check()?;
                if let Some(hit) = catalog
                    .lookup_member(&ctx, &ns, name.as_str(), cancel)?
                    .hit()
                {
                    names.add(
                        name.clone(),
                        candidate(
                            &unit,
                            Some(hit.target),
                            BindingOrigin::Prelude(ns.clone()),
                            Visibility::Private,
                            None,
                        ),
                    );
                }
            }
        } else if preludes.len() > 1 {
            result.diagnostics.push(
                Diagnostic::error(DiagnosticKind::DuplicateDeclaration {
                    name: "installed prelude".into(),
                })
                .with_span(Span::default()),
            );
        }
    }
    result.path_hits.retain(|(_, hit)| !hit.via.iter().any(|origin| matches!(origin, BindingOrigin::NamedImport(id) if id.unit == unit && result.directives.get(id.slot as usize).is_some_and(|directive| matches!(directive.resolution, DirectiveResolution::Ambiguous)))));
    result.dependencies = dependencies.into_iter().collect();
    result.scope = Arc::new(names);
    Ok(result)
}
fn add_dependencies(hit: &LookupHit, dependencies: &mut BTreeSet<ModuleIdentity>) {
    for origin in &hit.via {
        match origin {
            BindingOrigin::Declaration(source) => {
                dependencies.insert(source.unit.module.clone());
            }
            BindingOrigin::ModuleDeclaration { unit, .. }
            | BindingOrigin::NamedImport(DirectiveId { unit, .. })
            | BindingOrigin::GlobImport(DirectiveId { unit, .. }) => {
                dependencies.insert(unit.module.clone());
            }
            _ => {}
        }
    }
    match &hit.target {
        ResolvedTarget::Source(source)
        | ResolvedTarget::Namespace(NamespaceId::Associated(source)) => {
            dependencies.insert(source.unit.module.clone());
        }
        ResolvedTarget::Namespace(NamespaceId::Module(unit)) => {
            dependencies.insert(unit.module.clone());
        }
        _ => {}
    }
}
fn visibility_covers(
    source: Visibility,
    owner: &ModuleIdentity,
    exported: Visibility,
    exporter: &ModuleIdentity,
) -> bool {
    if source == Visibility::Public {
        return true;
    }
    if exported == Visibility::Public || owner.package != exporter.package {
        return false;
    }
    let source_scope = if source == Visibility::Private {
        owner.path.len()
    } else {
        owner.path.len().saturating_sub(1)
    };
    let export_scope = if exported == Visibility::Private {
        exporter.path.len()
    } else {
        exporter.path.len().saturating_sub(1)
    };
    exporter.path[..export_scope].starts_with(&owner.path[..source_scope])
}
fn reexport_allowed(
    catalog: &NamespaceCatalog,
    target: &ResolvedTarget,
    visibility: Visibility,
    exporter: &ModuleIdentity,
) -> bool {
    let candidates = catalog
        .namespaces
        .values()
        .flat_map(|t| t.names.entries.values())
        .flat_map(|e| &e.strong)
        .filter(|c| {
            c.target.as_ref() == Some(target)
                && matches!(
                    c.origin,
                    BindingOrigin::Declaration(_) | BindingOrigin::ModuleDeclaration { .. }
                )
        })
        .collect::<Vec<_>>();
    candidates.is_empty()
        || candidates
            .iter()
            .any(|c| visibility_covers(c.visibility, &c.owner, visibility, exporter))
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
                    return path.into();
                }
                base.pop();
            }
        }
        _ => return path.into(),
    }
    base.extend(segments.map(str::to_owned));
    format!("{}::{}", current.package.0, base.join("::"))
}
