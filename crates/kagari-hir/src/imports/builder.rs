//! Mutable fixed-point preparation; immutable publication contains no pending edges.
use crate::{
    hir::{
        item::{function::FunctionKind, storage::ExportItem},
        ty::TypeKind,
    },
    host::HostDeclarations,
    imports::{
        BindingCandidate, BindingOrigin, LocalName, ModuleGraph, ModuleImportFacts, ModuleNode,
        ModuleOrderError, NamespaceId, ResolvedTarget, SourceDeclRef, SourceItem, SourceUnit,
        catalog::{NamespaceCatalog, NamespaceTable},
        resolve::resolve_imports,
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
use kagari_types::{declaration::names::NameNamespace, visibility::Visibility};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

impl ModuleGraph {
    /// Builds the source input set's declarations, import fixed point and immutable catalog.
    ///
    /// Seeds declarations/package prefixes, then resolves every source against the
    /// previous pass's catalog into fresh tables. The inclusive range
    /// `0..=base.modules.len() * 2 + 1` permits at most `2N + 2` passes, where `N` is
    /// the number of distinct logical module identities, not the import edge count.
    /// Equality of the complete catalog ends the loop early. At the bound the current
    /// implementation publishes the last pass; it has no separate exhaustion result.
    /// SA2/SA3 in the repository review own scheduling/exhaustion follow-ups.
    ///
    /// For `api` re-exporting `math::*` and `app` importing `api::sum`, `api` can gain
    /// `sum` in one pass and `app` see that new binding only in the next. A pass scans
    /// all supplied modules, including installed inputs. Cancellation aborts the build.
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

    /// Returns facts for an exact source unit, retaining duplicate logical modules separately.
    pub(crate) fn imports_for(&self, unit: &SourceUnit) -> Option<&Arc<ModuleImportFacts>> {
        self.source_facts.get(unit)
    }

    /// Finds the representative graph node for a logical identity, or `None` if absent.
    pub fn node(&self, module: &ModuleIdentity) -> Option<&ModuleNode> {
        self.nodes.get(module)
    }

    /// Visits representative nodes in ordered logical-identity order.
    pub fn modules(&self) -> impl Iterator<Item = (&ModuleIdentity, &ModuleNode)> {
        self.nodes.iter()
    }

    /// Collects the root and transitive dependencies in sorted identity order.
    ///
    /// This is not a dependency-before-user topological order; a visited set terminates
    /// cycles. Every visited node must have no import diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a missing-node, invalid-import or cancellation error.
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

pub(super) fn location(module: &LoweredModule, span: Span) -> FileSpan {
    module.source.span(span).unwrap_or(FileSpan {
        file: module.source.origin_id(),
        revision: module.source.revision(),
        range: span,
    })
}

pub(super) fn candidate(
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
            item.namespace(),
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
                    NameNamespace::Type,
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
                    NameNamespace::Type,
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
