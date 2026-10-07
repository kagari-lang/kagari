//! Rebuild one module's contributions from an immutable catalog observation.
use crate::{
    hir::item::module::ImportLeaf,
    host::HostDeclarations,
    imports::{
        BindingOrigin, DirectiveId, DirectiveResolution, ImportDirective, ImportKind, LocalName,
        ModuleImportFacts, NamespaceId, ResolvedTarget, SourceUnit,
        bindings::{LookupOutcome, PerNamespace},
        builder::{candidate, location},
        catalog::{LookupContext, LookupHit, LookupResult, NamespaceCatalog, NamespaceResult},
        solver::Observation,
    },
    lower::LoweredModule,
    resolver::table::NameTable,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::ModuleIdentity,
    span::Span,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{declaration::names::NameNamespace, visibility::Visibility};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashSet},
    slice,
    sync::Arc,
};

struct ImportContext<'a> {
    lookup: LookupContext<'a>,
    catalog: &'a NamespaceCatalog,
    unit: &'a SourceUnit,
    cancel: &'a CancellationToken,
}

pub(super) struct ResolutionInputs<'a> {
    pub base: &'a NamespaceCatalog,
    pub catalog: &'a NamespaceCatalog,
    pub sources: &'a [&'a LoweredModule],
    pub hosts: &'a HostDeclarations,
    pub cancel: &'a CancellationToken,
    pub closed: &'a HashSet<(DirectiveId, NameNamespace)>,
    pub observations: &'a RefCell<HashSet<Observation>>,
}

pub(super) fn resolve_imports(
    module: &LoweredModule,
    inputs: &ResolutionInputs<'_>,
) -> Result<ModuleImportFacts, Cancelled> {
    let ResolutionInputs {
        base,
        catalog,
        sources,
        hosts,
        cancel,
        closed,
        observations,
    } = inputs;
    let unit = SourceUnit::of(module);
    let ctx = LookupContext {
        importer: &unit.module,
        hosts,
        observations: Some(observations),
        avoid: None,
    };
    let context = ImportContext {
        lookup: ctx,
        catalog,
        unit: &unit,
        cancel,
    };
    let mut names = (*base.namespaces[&NamespaceId::Module(unit.clone())].names).clone();
    let mut result = ModuleImportFacts::default();
    let mut dependencies = BTreeSet::from_iter(module.native_dependencies.iter().cloned());
    for source in *sources {
        result
            .array_interfaces
            .extend(source.native_array_interfaces.clone());
    }
    for (slot, import) in module.module.imports.iter().enumerate() {
        cancel.check()?;
        let id = DirectiveId {
            unit: unit.clone(),
            slot: u32::try_from(slot).expect("import capacity"),
        };
        let kind = match &import.kind {
            ImportLeaf::Named { alias } => ImportKind::Named {
                alias: alias.as_deref().and_then(LocalName::new),
            },
            ImportLeaf::Glob => ImportKind::Glob,
        };
        let mut directive = ImportDirective {
            id: id.clone(),
            path: import.path.clone(),
            kind,
            span: location(module, import.span),
            root_span: location(module, import.root_span),
            visibility: import.visibility,
            resolution: match import.kind {
                ImportLeaf::Named { .. } => DirectiveResolution::Named(PerNamespace {
                    types: LookupOutcome::Absent,
                    values: LookupOutcome::Absent,
                }),
                ImportLeaf::Glob => DirectiveResolution::Glob(LookupOutcome::Pending),
            },
            direct_dependencies: BTreeSet::new(),
        };
        let path = normalize_import_path(&directive.path, &unit.module);
        catalog.path_dependencies(&path, &mut directive.direct_dependencies);
        let categories: &[NameNamespace] =
            if let Some(namespace) = module.native_import_namespaces.get(&slot) {
                slice::from_ref(namespace)
            } else if matches!(import.kind, ImportLeaf::Glob) {
                &[NameNamespace::Type]
            } else {
                &NameNamespace::ALL
            };
        for &namespace in categories {
            let lookup_context = LookupContext {
                importer: &unit.module,
                hosts,
                observations: Some(observations),
                avoid: Some((id.clone(), namespace)),
            };
            let directive_context = ImportContext {
                lookup: lookup_context,
                catalog,
                unit: &unit,
                cancel,
            };
            let ctx = &directive_context.lookup;
            let mut lookup = catalog.absolute(ctx, &path, namespace, cancel)?;
            if matches!(import.kind, ImportLeaf::Glob)
                && matches!(lookup, LookupResult::Missing)
                && matches!(
                    catalog.absolute(ctx, &path, NameNamespace::Value, cancel)?,
                    LookupResult::Found(_)
                )
            {
                lookup = LookupResult::NotNamespace;
            }
            let mut support = HashSet::new();
            let outcome = match lookup {
                LookupResult::Found(hit)
                    if matches!(import.kind, ImportLeaf::Glob)
                        || reexport_allowed(
                            catalog,
                            &hit.target,
                            import.visibility,
                            &unit.module,
                        ) =>
                {
                    support = hit.support.clone();
                    support.insert((id.clone(), namespace));
                    add_dependencies(&hit, &mut directive.direct_dependencies);
                    if matches!(import.kind, ImportLeaf::Glob)
                        && expand_glob(
                            &directive_context,
                            &directive,
                            &hit,
                            &mut names,
                            &mut result.diagnostics,
                            closed.contains(&(id.clone(), namespace)),
                        )?
                    {
                        LookupOutcome::Pending
                    } else {
                        LookupOutcome::Resolved(hit.target)
                    }
                }
                LookupResult::Missing => LookupOutcome::Absent,
                LookupResult::Unresolved if closed.contains(&(id.clone(), namespace)) => {
                    LookupOutcome::Absent
                }
                LookupResult::Unresolved => LookupOutcome::Pending,
                LookupResult::Ambiguous(_) => LookupOutcome::Ambiguous,
                LookupResult::Found(_) | LookupResult::Inaccessible(_) => {
                    LookupOutcome::Inaccessible
                }
                LookupResult::NotNamespace => LookupOutcome::NotNamespace,
                LookupResult::StaleSource => LookupOutcome::StaleSource,
            };
            if let Some(name) = import.local_name().and_then(LocalName::new)
                && outcome != LookupOutcome::Absent
            {
                let mut binding = candidate(
                    &unit,
                    None,
                    BindingOrigin::NamedImport(id.clone()),
                    import.visibility,
                    Some(directive.span),
                );
                binding.resolution = outcome.clone();
                binding.support = support;
                names.add(namespace, name, binding);
            }
            if matches!(import.kind, ImportLeaf::Glob) && outcome == LookupOutcome::Pending {
                let mut binding = candidate(
                    &unit,
                    None,
                    BindingOrigin::GlobImport(id.clone()),
                    import.visibility,
                    Some(directive.span),
                );
                binding.resolution = LookupOutcome::Pending;
                names.pending_globs.push(binding);
            }
            match &mut directive.resolution {
                DirectiveResolution::Named(outcomes) => outcomes[namespace] = outcome,
                DirectiveResolution::Glob(result) => *result = outcome,
            }
        }
        let outcomes = match &directive.resolution {
            DirectiveResolution::Named(outcomes) => vec![&outcomes.types, &outcomes.values],
            DirectiveResolution::Glob(outcome) => vec![outcome],
        };
        let mut failures = Vec::new();
        for outcome in &outcomes {
            let kind = match outcome {
                LookupOutcome::Ambiguous => {
                    Some(DiagnosticKind::AmbiguousImport { path: path.clone() })
                }
                LookupOutcome::Inaccessible => {
                    Some(DiagnosticKind::ImportNotPublic { path: path.clone() })
                }
                LookupOutcome::NotNamespace if matches!(import.kind, ImportLeaf::Glob) => {
                    Some(DiagnosticKind::InvalidGlobTarget { path: path.clone() })
                }
                LookupOutcome::NotNamespace | LookupOutcome::StaleSource => {
                    Some(DiagnosticKind::UnknownName { name: path.clone() })
                }
                _ => None,
            };
            if let Some(kind) = kind
                && !failures.contains(&kind)
            {
                failures.push(kind);
            }
        }
        if outcomes
            .iter()
            .all(|outcome| **outcome == LookupOutcome::Absent)
        {
            failures.push(DiagnosticKind::UnknownName { name: path.clone() });
        }
        result.diagnostics.extend(
            failures
                .into_iter()
                .map(|kind| Diagnostic::error(kind).with_span(directive.span.range)),
        );
        // Prefix navigation is Type-only; a named terminal retains every category.
        for (prefix, span) in module.source_map.import_path(slot) {
            let terminal = prefix == &directive.path;
            for &namespace in if terminal {
                categories
            } else {
                &[NameNamespace::Type]
            } {
                let ctx = LookupContext {
                    importer: &unit.module,
                    hosts,
                    observations: Some(observations),
                    avoid: Some((
                        id.clone(),
                        if matches!(directive.kind, ImportKind::Glob) {
                            NameNamespace::Type
                        } else {
                            namespace
                        },
                    )),
                };
                if let Some(mut hit) = catalog
                    .absolute(
                        &ctx,
                        &normalize_import_path(prefix, &unit.module),
                        namespace,
                        cancel,
                    )?
                    .hit()
                {
                    hit.via.insert(
                        0,
                        if matches!(directive.kind, ImportKind::Glob) {
                            BindingOrigin::GlobImport(id.clone())
                        } else {
                            BindingOrigin::NamedImport(id.clone())
                        },
                    );
                    result.path_hits.push((location(module, *span), hit));
                }
            }
        }
        dependencies.extend(directive.direct_dependencies.iter().cloned());
        result.directives.push(directive);
    }
    diagnose_bindings(&mut names, &mut result, &mut dependencies);
    add_implicit(
        module,
        sources,
        &context,
        &mut names,
        &mut result,
        &mut dependencies,
    )?;
    result.dependencies = dependencies.into_iter().collect();
    result.scope = Arc::new(names);
    Ok(result)
}

fn expand_glob(
    context: &ImportContext<'_>,
    directive: &ImportDirective,
    hit: &LookupHit,
    names: &mut NameTable,
    diagnostics: &mut Vec<Diagnostic>,
    closed: bool,
) -> Result<bool, Cancelled> {
    let ImportContext {
        lookup: ctx,
        catalog,
        unit,
        cancel,
    } = context;
    let ns = catalog.namespace_of(ctx, &hit.target, cancel)?;
    if let NamespaceResult::Found(namespace) = &ns {
        ctx.observe(namespace);
    }
    let members = match &ns {
        NamespaceResult::Found(NamespaceId::Host(id)) => ctx
            .hosts
            .members_of_module(*id)
            .into_iter()
            .map(|(name, _)| name)
            .collect::<BTreeSet<_>>(),
        NamespaceResult::Found(ns)
            if catalog
                .namespaces
                .get(ns)
                .is_some_and(|table| table.glob_allowed) =>
        {
            catalog.namespaces[ns]
                .names
                .entries
                .keys()
                .map(|name| name.as_str().to_owned())
                .collect()
        }
        _ => {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidGlobTarget {
                    path: directive.path.clone(),
                })
                .with_span(directive.span.range),
            );
            return Ok(false);
        }
    };
    let NamespaceResult::Found(ns) = ns else {
        unreachable!()
    };
    let mut pending = !closed
        && catalog
            .namespaces
            .get(&ns)
            .is_some_and(|table| !table.names.pending_globs.is_empty());
    for name in members {
        for namespace in NameNamespace::ALL {
            let lookup = catalog.lookup_member(ctx, &ns, &name, namespace, cancel)?;
            if matches!(lookup, LookupResult::Unresolved) {
                pending |= !closed;
            }
            if matches!(lookup, LookupResult::Ambiguous(_)) {
                let mut binding = candidate(
                    unit,
                    None,
                    BindingOrigin::GlobImport(directive.id.clone()),
                    directive.visibility,
                    Some(directive.span),
                );
                binding.resolution = LookupOutcome::Ambiguous;
                names.add(
                    namespace,
                    LocalName::new(&name).expect("member name"),
                    binding,
                );
            }
            if let Some(member) = lookup.hit()
                && reexport_allowed(catalog, &member.target, directive.visibility, &unit.module)
            {
                let mut binding = candidate(
                    unit,
                    Some(member.target),
                    BindingOrigin::GlobImport(directive.id.clone()),
                    directive.visibility,
                    Some(directive.span),
                );
                binding.support = member.support;
                binding.support.extend(hit.support.iter().cloned());
                binding
                    .support
                    .insert((directive.id.clone(), NameNamespace::Type));
                names.add(
                    namespace,
                    LocalName::new(&name).expect("member name"),
                    binding,
                );
            }
        }
    }
    Ok(pending)
}

fn diagnose_bindings(
    names: &mut NameTable,
    result: &mut ModuleImportFacts,
    dependencies: &mut BTreeSet<ModuleIdentity>,
) {
    for (name, slots) in &mut names.entries {
        for namespace in NameNamespace::ALL {
            let entry = &mut slots[namespace];
            let strong_conflict = entry.strong.len() > 1
                && entry
                    .strong
                    .iter()
                    .all(|candidate| candidate.resolution != LookupOutcome::Pending);
            let conflict = strong_conflict
                || (entry.strong.is_empty()
                    && matches!(
                        NameTable::select(&entry.globs, false),
                        LookupResult::Ambiguous(_)
                    ));
            let imported = if entry.strong.is_empty() {
                !entry.globs.is_empty()
            } else {
                entry
                    .strong
                    .iter()
                    .any(|c| matches!(c.origin, BindingOrigin::NamedImport(_)))
            };
            if conflict && imported {
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
                            .map_or(Span::default(), |site| site.range),
                    ),
                );
            }
            for candidate in &mut entry.strong {
                if strong_conflict
                    && let BindingOrigin::NamedImport(id) = &candidate.origin
                    && let DirectiveResolution::Named(outcomes) =
                        &mut result.directives[id.slot as usize].resolution
                {
                    outcomes[namespace] = LookupOutcome::Ambiguous;
                    candidate.resolution = LookupOutcome::Ambiguous;
                }
                if matches!(candidate.origin, BindingOrigin::ModuleDeclaration { .. }) {
                    if let Some(ResolvedTarget::Namespace(NamespaceId::Module(child))) =
                        candidate.resolution.target()
                    {
                        dependencies.insert(child.module.clone());
                    } else {
                        result.diagnostics.push(
                            Diagnostic::error(DiagnosticKind::UnknownName {
                                name: name.as_str().into(),
                            })
                            .with_span(
                                candidate
                                    .location
                                    .map_or(Span::default(), |site| site.range),
                            ),
                        );
                    }
                }
            }
        }
    }
    result.path_hits.retain(|(_, hit)| !hit.via.iter().any(|origin| {
        let BindingOrigin::NamedImport(id) = origin else { return false; };
        names.unit.as_ref() == Some(&id.unit) && result.directives.get(id.slot as usize).is_some_and(|directive| {
            matches!(&directive.resolution, DirectiveResolution::Named(outcomes) if outcomes[hit.namespace] == LookupOutcome::Ambiguous)
        })
    }));
}

fn add_implicit(
    module: &LoweredModule,
    sources: &[&LoweredModule],
    context: &ImportContext<'_>,
    names: &mut NameTable,
    result: &mut ModuleImportFacts,
    dependencies: &mut BTreeSet<ModuleIdentity>,
) -> Result<(), Cancelled> {
    let ImportContext {
        lookup: ctx,
        catalog,
        unit,
        cancel,
    } = context;
    for source in sources.iter().filter(|source| source.registered_native_api) {
        let installed = SourceUnit::of(source);
        if !module.registered_native_api {
            dependencies.insert(installed.module.clone());
        }
        let alias = source
            .native_package_alias
            .as_deref()
            .unwrap_or(&installed.module.package.0);
        if let Some(name) = LocalName::new(alias) {
            let ns = NamespaceId::InstalledPrefix(ModuleIdentity {
                package: installed.module.package.clone(),
                path: vec![],
            });
            let candidate = candidate(
                unit,
                Some(ResolvedTarget::Namespace(ns)),
                BindingOrigin::Package(installed.module.package.clone()),
                Visibility::Private,
                None,
            );
            if !names.entries.entry(name.clone()).or_default()[NameNamespace::Type]
                .implicit
                .contains(&candidate)
            {
                names.add(NameNamespace::Type, name, candidate);
            }
        }
    }
    if !module.registered_native_api {
        let preludes = sources
            .iter()
            .filter(|source| source.native_prelude)
            .collect::<Vec<_>>();
        if let [prelude] = preludes.as_slice() {
            let ns = NamespaceId::Module(SourceUnit::of(prelude));
            ctx.observe(&ns);
            for name in catalog.namespaces[&ns].names.entries.keys() {
                for namespace in NameNamespace::ALL {
                    if let Some(hit) = catalog
                        .lookup_member(ctx, &ns, name.as_str(), namespace, cancel)?
                        .hit()
                    {
                        let mut binding = candidate(
                            unit,
                            Some(hit.target),
                            BindingOrigin::Prelude(ns.clone()),
                            Visibility::Private,
                            None,
                        );
                        binding.support = hit.support;
                        names.add(namespace, name.clone(), binding);
                    }
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
    Ok(())
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
        .flat_map(|slots| slots.iter().flat_map(|(_, entry)| &entry.strong))
        .filter(|c| {
            c.resolution.target() == Some(target)
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
